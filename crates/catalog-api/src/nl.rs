// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Natural-language search, read by Claude when a key is configured.
//!
//! The built-in rules in `catalog_core::nl` handle the common phrasings. Longer, looser
//! queries ("something like Overleaf that Swedish universities can use without
//! sending names") go to Claude, which answers in a JSON schema whose enums are the
//! catalog's own federations, countries, categories, and attributes, so it can only
//! choose filters that exist. Its answer is validated again anyway, and any failure
//! falls back to the rules.
//!
//! Environment (credentials are read by `catalog_claude`):
//! - `CATALOG_NL_MODEL` (default `claude-opus-5-5`), `CATALOG_NL_EFFORT` (default `low`)
//! - `CATALOG_NL_ENGINE` — `auto` (default), `rules` to never call Claude

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};
use catalog_claude::Claude;
use catalog_core::categories::CATEGORIES;
use catalog_core::environment::Environment;
use catalog_core::model::Availability;
use catalog_core::nl::{Interpretation, InterpretedParams, MatchedTerm, Vocab};
use catalog_core::regions::{RegionKind, REGIONS};
use catalog_core::search::Catalog;
use catalog_core::sector::Sector;
use catalog_core::sharing::Sharing;
use serde::Deserialize;
use serde_json::{json, Value};

/// How many words before a query is worth a model call. Shorter queries are names
/// or topics, which the rules read just as well, for free.
const MIN_WORDS_FOR_MODEL: usize = 3;
const CACHE_LIMIT: usize = 2000;
const MAX_ATTRIBUTES: usize = 80;

pub struct ClaudeReader {
    claude: Claude,
    // (snapshot generated_at, normalized query) → reading. Saves repeat calls for
    // popular queries; cleared when it fills.
    cache: Mutex<HashMap<(String, String), Interpretation>>,
}

impl ClaudeReader {
    pub async fn from_env() -> Result<Option<Self>> {
        if std::env::var("CATALOG_NL_ENGINE").as_deref() == Ok("rules") {
            return Ok(None);
        }
        let claude = Claude::from_env(
            "CATALOG_NL_MODEL",
            "CATALOG_NL_EFFORT",
            Duration::from_secs(12),
        )
        .await?;
        Ok(claude.map(Self::new))
    }

    fn new(claude: Claude) -> Self {
        Self {
            claude,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn model(&self) -> &str {
        self.claude.model()
    }

    async fn read(&self, catalog: &Catalog, query: &str) -> Result<Interpretation> {
        let cache_key = (
            catalog.snapshot().generated_at.clone(),
            query.trim().to_lowercase(),
        );
        if let Some(hit) = self.cache.lock().unwrap().get(&cache_key) {
            return Ok(hit.clone());
        }

        let vocab = catalog.vocab();
        // The system prompt is the vocabulary: stable per snapshot, so it caches
        // across every query until the next ingest.
        let answer = self
            .claude
            .structured(&system_prompt(vocab), query, schema(vocab), 4096)
            .await?;
        let out: ModelReading = serde_json::from_value(answer.value)
            .context("model output did not match the schema")?;
        let served_by = answer.model;

        let mut notes = Vec::new();
        let federations: Vec<String> = out.federations.into_iter().flatten().collect();
        let params = vocab.validate(
            InterpretedParams {
                q: Some(out.keywords),
                federation: (!federations.is_empty()).then(|| federations.join(",")),
                country: Some(
                    out.countries
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(","),
                )
                .filter(|c| !c.is_empty()),
                region: (!out.regions.is_empty()).then(|| out.regions.join(",")),
                environment: (!out.environments.is_empty()).then(|| out.environments.join(",")),
                sharing: (!out.sharing.is_empty()).then(|| out.sharing.join(",")),
                sector: (!out.sectors.is_empty()).then(|| out.sectors.join(",")),
                category: (!out.categories.is_empty()).then(|| out.categories.join(",")),
                availability: out.availability,
                attribute: out.attribute,
                source: out.source,
                provider_type: out.provider_type,
            },
            &mut notes,
        );
        let reading = Interpretation {
            query: query.trim().to_owned(),
            engine: served_by,
            matched: matched_from(vocab, &params),
            params,
            notes,
            explanation: Some(out.explanation).filter(|e| !e.trim().is_empty()),
        };

        let mut cache = self.cache.lock().unwrap();
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(cache_key, reading.clone());
        Ok(reading)
    }
}

/// `engine`: `auto` (default), `rules`, or `claude`.
pub async fn interpret(
    reader: Option<&ClaudeReader>,
    catalog: &Catalog,
    query: &str,
    engine: &str,
) -> Interpretation {
    let words = query.split_whitespace().count();
    let want_model = match engine {
        "rules" => false,
        "claude" => true,
        _ => words >= MIN_WORDS_FOR_MODEL,
    };
    let Some(reader) = reader.filter(|_| want_model) else {
        let mut i = catalog.interpret(query);
        if engine == "claude" {
            i.notes.push(
                "Claude is not configured for this API, so the built-in rules read this query."
                    .into(),
            );
        }
        return i;
    };
    match reader.read(catalog, query).await {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(error = %e, "Claude reading failed; using rules");
            let mut i = catalog.interpret(query);
            i.notes.push(
                "Claude could not read this query just now, so the built-in rules did.".into(),
            );
            i
        }
    }
}

#[derive(Deserialize)]
struct ModelReading {
    keywords: String,
    /// `null` items only when the catalog lists no federations (see [`enum_array`]).
    federations: Vec<Option<String>>,
    /// `null` items only when the catalog lists no countries (see [`enum_array`]).
    #[serde(default)]
    countries: Vec<Option<String>>,
    #[serde(default)]
    regions: Vec<String>,
    #[serde(default)]
    environments: Vec<String>,
    #[serde(default)]
    sharing: Vec<String>,
    #[serde(default)]
    sectors: Vec<String>,
    categories: Vec<String>,
    availability: Option<String>,
    attribute: Option<String>,
    source: Option<String>,
    provider_type: Option<String>,
    explanation: String,
}

fn matched_from(vocab: &Vocab, p: &InterpretedParams) -> Vec<MatchedTerm> {
    let mut out = Vec::new();
    let mut push = |field: &str, value: &str| {
        out.push(MatchedTerm {
            text: String::new(),
            field: field.into(),
            value: value.into(),
            label: vocab.label(field, value),
        })
    };
    for (field, v) in [
        ("availability", &p.availability),
        ("attribute", &p.attribute),
        ("source", &p.source),
        ("provider_type", &p.provider_type),
    ] {
        if let Some(v) = v {
            push(field, v);
        }
    }
    for f in p.federation.iter().flat_map(|f| f.split(',')) {
        push("federation", f);
    }
    for c in p.category.iter().flat_map(|c| c.split(',')) {
        push("category", c);
    }
    for e in p.environment.iter().flat_map(|e| e.split(',')) {
        push("environment", e);
    }
    for v in p.sharing.iter().flat_map(|v| v.split(',')) {
        push("sharing", v);
    }
    for v in p.sector.iter().flat_map(|v| v.split(',')) {
        push("sector", v);
    }
    for c in p.country.iter().flat_map(|c| c.split(',')) {
        push("country", c);
    }
    for r in p.region.iter().flat_map(|r| r.split(',')) {
        push("region", r);
    }
    out
}

fn nullable_enum(values: Vec<String>) -> Value {
    if values.is_empty() {
        json!({ "type": "null" })
    } else {
        json!({ "anyOf": [{ "type": "string", "enum": values }, { "type": "null" }] })
    }
}

/// Any number of `values`; an empty enum is not a valid schema, so with no
/// values the only answer is an empty list (or nulls, which are ignored).
fn enum_array(values: Vec<String>) -> Value {
    let items = if values.is_empty() {
        json!({ "type": "null" })
    } else {
        json!({ "type": "string", "enum": values })
    };
    json!({ "type": "array", "items": items })
}

fn attributes(vocab: &Vocab) -> impl Iterator<Item = &(String, Option<String>)> {
    vocab.attributes.iter().take(MAX_ATTRIBUTES)
}

fn schema(vocab: &Vocab) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["keywords", "federations", "countries", "regions", "environments", "sharing", "sectors", "categories", "availability", "attribute", "source", "provider_type", "explanation"],
        "properties": {
            "keywords": { "type": "string" },
            "federations": enum_array(vocab.federations.keys().cloned().collect()),
            "countries": enum_array(vocab.countries.keys().cloned().collect()),
            "regions": { "type": "array", "items": { "type": "string", "enum": REGIONS.iter().map(|r| r.key.as_str()).collect::<Vec<_>>() } },
            "environments": enum_array(Environment::ALL.iter().map(|e| e.key().to_owned()).collect()),
            "sharing": enum_array(Sharing::ALL.iter().map(|s| s.key().to_owned()).collect()),
            "sectors": enum_array(Sector::ALL.iter().map(|s| s.key().to_owned()).chain(["unknown".to_owned()]).collect()),
            "categories": { "type": "array", "items": { "type": "string", "enum": CATEGORIES.iter().map(|c| c.key).collect::<Vec<_>>() } },
            "availability": nullable_enum(Availability::ALL.iter().map(|a| a.key().to_owned()).collect()),
            "attribute": nullable_enum(attributes(vocab).map(|(n, _)| n.clone()).collect()),
            "source": nullable_enum(vec!["incommon".into(), "edugain".into()]),
            "provider_type": nullable_enum(vocab.provider_types.keys().cloned().collect()),
            "explanation": { "type": "string" },
        },
    })
}

fn system_prompt(vocab: &Vocab) -> String {
    let mut s = String::from(
        "You read searches typed into a catalog of federated-login services and turn them into the catalog's filters.

The catalog lists SAML service providers from InCommon, the US research and education federation, and from eduGAIN, which connects national federations worldwide. Each service was registered by one federation.

Set a filter only when the search clearly asks for it, and leave it null otherwise. Most searches are just a name or a topic: return those as keywords with no filters. Put the words that describe what the service is or does (its subject, product, or vendor) in `keywords`, as a short space-separated list without filler words, and do not repeat words already used by a filter.

- `federations`: the registering federations, when the search names one or more (\"SWAMID\", \"the UK federation\", \"registered by InCommon\", \"SWAMID or DFN-AAI services\"). A service matches any of them, so list every federation the search names; leave the list empty when it names none.
- `countries`: the registering federation's countries, when the search names one or more countries or nationalities rather than a federation. A service registered in any of them matches. \"America\" alone usually means the United States.
- `regions`: when the search names a region or a grouping of countries (\"Europe\", \"the EU\", \"the Americas\", \"Asia-Pacific\", \"Schengen countries\", \"NATO members\", \"OECD\", \"BRICS\", \"GÉANT countries\"). Pick the region; do not list its countries. The EU, Schengen, and the euro area are different lists. Use a research network's region only when the search is about where its member networks are, not about the network's own services. A place word inside an organization's name (\"European Molecular Biology Laboratory\", \"American Chemical Society\") is a keyword, not a place.
- `environments`: only when the search asks for test, staging, or development instances (\"test instances of Canvas\", \"the staging environment\") or for live ones only (\"only production services\"); a service in any listed environment matches. To leave out test instances (\"no test sites\"), list `production` and `unknown`. \"Test\" or \"production\" as a subject (\"test prep\", \"video production\") is a keyword, not an environment. Leave the list empty otherwise.
- `sharing`: only when the search asks who a service is open to. `multilateral` for services any institution's users can use (\"services anyone in eduGAIN can use\"); `bilateral` for ones registered for a single institution (\"institution-specific\", \"per-campus\"). To leave out single-institution registrations (\"not institution-specific\"), list `multilateral` and `unknown`. Leave the list empty otherwise.
- `sectors`: only when the search asks who runs the service: the kind of organization that operates it (\"services from libraries\", \"run by museums\", \"commercial vendors\", \"government agencies\", \"universities\"). Not the audience, and not the subject: \"library management systems\" or \"museum ticketing\" are keywords. For \"non-commercial\", list every sector except `commercial` and `publishers`. Leave the list empty otherwise.
- `categories`: entity categories and certifications the service must carry, all of them.
- `availability`: where the service can be used from. Use it only for searches about reach (\"not in InCommon\", \"exported to eduGAIN\"); do not combine it with `source`.
- `source`: `incommon` when the search asks only for services InCommon members can use; `edugain` for services available through eduGAIN.
- `attribute`: when the search says the service requests, needs, or receives a particular user attribute (email, ORCID, name). Pick the attribute Name. A topic like \"email marketing\" is a keyword, not an attribute.
- `provider_type`: only when the search asks for that kind of provider and the value is listed.
- `explanation`: one short sentence for the person who searched, saying how you read it, starting with \"Services\".

Categories:
",
    );
    for c in CATEGORIES {
        s.push_str(&format!("- {}: {}. {}\n", c.key, c.label, c.description));
    }
    s.push_str("\nAvailability values:\n");
    for a in Availability::ALL {
        s.push_str(&format!("- {}: {}\n", a.key(), a.label()));
    }
    s.push_str("\nFederations (code: name, countries):\n");
    for (code, (name, ccs)) in &vocab.federations {
        s.push_str(&format!("- {code}: {name}, {}\n", ccs.join(" ")));
    }
    s.push_str("\nRegions (key: name; source):\n");
    for r in REGIONS.iter() {
        let dated = match (r.kind, r.as_of.as_deref()) {
            (RegionKind::Geographic, _) | (_, None) => String::new(),
            (_, Some(d)) => format!(", membership as of {d}"),
        };
        let note = r
            .note
            .as_deref()
            .map(|n| format!(" {n}"))
            .unwrap_or_default();
        s.push_str(&format!(
            "- {}: {}; {}{dated}.{note}\n",
            r.key, r.label, r.source
        ));
    }
    s.push_str("\nCountries (code: name):\n");
    for (cc, name) in &vocab.countries {
        s.push_str(&format!("- {cc}: {name}\n"));
    }
    s.push_str("\nRequested attributes (Name: friendly name):\n");
    for (name, friendly) in attributes(vocab) {
        s.push_str(&format!(
            "- {name}: {}\n",
            friendly.as_deref().unwrap_or("-")
        ));
    }
    if !vocab.provider_types.is_empty() {
        s.push_str("\nProvider types: ");
        s.push_str(
            &vocab
                .provider_types
                .values()
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        );
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex as StdMutex};

    use axum::extract::State;
    use axum::http::HeaderMap;
    use axum::routing::post;
    use axum::{Json, Router};
    use catalog_core::model::{Federation, RequestedAttribute, Service, Snapshot};

    use super::*;

    type Seen = Arc<StdMutex<Vec<(HeaderMap, Value)>>>;

    /// A stand-in for the Messages API that records requests and returns `reply`.
    async fn stub(reply: Value) -> (String, Seen) {
        let seen: Seen = Arc::default();
        let app = Router::new()
            .route(
                "/v1/messages",
                post(
                    |State((seen, reply)): State<(Seen, Value)>,
                     headers: HeaderMap,
                     Json(body): Json<Value>| async move {
                        seen.lock().unwrap().push((headers, body));
                        Json(reply)
                    },
                ),
            )
            .with_state((seen.clone(), reply));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}"), seen)
    }

    fn reader(base: String) -> ClaudeReader {
        let claude = Claude::new(
            "test-key".into(),
            "claude-opus-5-5".into(),
            "low".into(),
            Duration::from_secs(5),
        )
        .unwrap()
        .with_base(base);
        ClaudeReader::new(claude)
    }

    fn catalog() -> Catalog {
        let s = Service {
            entity_id: "https://sp.example.se/sp".into(),
            id: catalog_core::entity_hash("https://sp.example.se/sp"),
            requested_attributes: vec![RequestedAttribute {
                name: "urn:oid:0.9.2342.19200300.100.1.3".into(),
                friendly_name: Some("mail".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let swamid = Federation {
            code: "SWAMID".into(),
            name: "SWAMID".into(),
            registration_authority: "http://www.swamid.se/".into(),
            country_codes: vec!["SE".into()],
            countries: vec!["Sweden".into()],
            ..Default::default()
        };
        let dfn = Federation {
            code: "DFN-AAI".into(),
            name: "DFN-AAI".into(),
            registration_authority: "https://www.aai.dfn.de".into(),
            country_codes: vec!["DE".into()],
            countries: vec!["Germany".into()],
            ..Default::default()
        };
        Catalog::new(Snapshot {
            services: vec![s],
            federations: vec![swamid, dfn],
            ..Default::default()
        })
    }

    fn message(stop_reason: &str, text: &str) -> Value {
        json!({
            "id": "msg_test", "type": "message", "role": "assistant", "model": "claude-opus-5-5",
            "stop_reason": stop_reason,
            "content": [{ "type": "thinking", "thinking": "" }, { "type": "text", "text": text }],
            "usage": { "input_tokens": 1, "output_tokens": 1 },
        })
    }

    #[tokio::test]
    async fn claude_reading_is_requested_correctly_and_validated() {
        let answer = json!({
            "keywords": "collaborative writing", "federations": ["NOT-A-FED"], "countries": ["SE"], "regions": [], "environments": [], "sharing": [], "sectors": [],
            "categories": ["research-and-scholarship"], "availability": null,
            "attribute": "urn:oid:0.9.2342.19200300.100.1.3", "source": null, "provider_type": null,
            "explanation": "Services for collaborative writing in Sweden that request email.",
        });
        let (base, seen) = stub(message("end_turn", &answer.to_string())).await;
        let r = reader(base);
        let c = catalog();
        let i = interpret(
            Some(&r),
            &c,
            "R&S collaborative writing tools in Sweden that need email",
            "auto",
        )
        .await;

        assert_eq!(i.engine, "claude-opus-5-5");
        assert_eq!(i.params.country.as_deref(), Some("SE"));
        assert_eq!(
            i.params.category.as_deref(),
            Some("research-and-scholarship")
        );
        assert_eq!(i.params.q.as_deref(), Some("collaborative writing"));
        assert_eq!(
            i.params.federation, None,
            "values outside the catalog are dropped"
        );
        assert!(i.notes.iter().any(|n| n.contains("NOT-A-FED")));
        assert!(i.explanation.is_some());

        let (headers, body) = seen.lock().unwrap()[0].clone();
        assert_eq!(headers["x-api-key"], "test-key");
        assert_eq!(headers["anthropic-version"], "2026-10-21");
        assert_eq!(headers["anthropic-beta"], catalog_claude::FALLBACK_BETA);
        assert_eq!(body["fallbacks"], "default");
        assert_eq!(body["output_config"]["effort"], "low");
        assert_eq!(body["output_config"]["format"]["type"], "json_schema");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert!(body.get("thinking").is_none() && body.get("temperature").is_none());
        let fed_enum = &body["output_config"]["format"]["schema"]["properties"]["federations"]
            ["items"]["enum"];
        assert_eq!(fed_enum, &json!(["DFN-AAI", "SWAMID"]));

        // A repeat of the same query is served from the cache.
        interpret(
            Some(&r),
            &c,
            "r&s collaborative writing tools in sweden that need email",
            "auto",
        )
        .await;
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn claude_can_name_several_federations() {
        let answer = json!({
            "keywords": "", "federations": ["SWAMID", "DFN-AAI", "NOPE"], "countries": [], "regions": [], "environments": [], "sharing": [], "sectors": [],
            "categories": [], "availability": null, "attribute": null, "source": null,
            "provider_type": null, "explanation": "Services registered by SWAMID or DFN-AAI.",
        });
        let (base, _) = stub(message("end_turn", &answer.to_string())).await;
        let i = interpret(
            Some(&reader(base)),
            &catalog(),
            "services from SWAMID or the German federation",
            "auto",
        )
        .await;
        assert_eq!(i.params.federation.as_deref(), Some("SWAMID,DFN-AAI"));
        let feds: Vec<&str> = i
            .matched
            .iter()
            .filter(|m| m.field == "federation")
            .map(|m| m.value.as_str())
            .collect();
        assert_eq!(feds, ["SWAMID", "DFN-AAI"]);
        assert!(i.notes.iter().any(|n| n.contains("NOPE")));
    }

    #[tokio::test]
    async fn claude_can_ask_for_environments() {
        let answer = json!({
            "keywords": "canvas", "federations": [], "countries": [], "regions": [],
            "environments": ["test", "staging", "prod"], "categories": [], "availability": null,
            "attribute": null, "source": null, "provider_type": null,
            "explanation": "Test and staging instances of Canvas.",
        });
        let (base, seen) = stub(message("end_turn", &answer.to_string())).await;
        let i = interpret(
            Some(&reader(base)),
            &catalog(),
            "canvas test and staging copies",
            "auto",
        )
        .await;
        assert_eq!(i.params.environment.as_deref(), Some("test,staging"));
        let envs: Vec<&str> = i
            .matched
            .iter()
            .filter(|m| m.field == "environment")
            .map(|m| m.label.as_str())
            .collect();
        assert_eq!(envs, ["Test", "Staging"]);
        assert!(i.notes.iter().any(|n| n.contains("prod")));

        let body = seen.lock().unwrap()[0].1.clone();
        let schema = &body["output_config"]["format"]["schema"];
        assert_eq!(
            schema["properties"]["environments"]["items"]["enum"],
            json!(["production", "test", "staging", "development", "unknown"])
        );
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("environments")));
    }

    #[tokio::test]
    async fn claude_names_regions_rather_than_country_lists() {
        let answer = json!({
            "keywords": "", "federations": [], "countries": ["SE", "XX"],
            "regions": ["europe", "atlantis"], "environments": [], "sharing": ["multilateral"], "sectors": ["libraries", "banks"], "categories": [],
            "availability": null, "attribute": null, "source": null, "provider_type": null,
            "explanation": "Services registered in Sweden or anywhere in Europe.",
        });
        let (base, seen) = stub(message("end_turn", &answer.to_string())).await;
        let i = interpret(
            Some(&reader(base)),
            &catalog(),
            "services from Sweden or elsewhere in Europe",
            "auto",
        )
        .await;
        assert_eq!(i.params.country.as_deref(), Some("SE"));
        assert_eq!(i.params.region.as_deref(), Some("europe"));
        assert_eq!(i.params.sharing.as_deref(), Some("multilateral"));
        assert_eq!(i.params.sector.as_deref(), Some("libraries"));
        let labels: Vec<&str> = i.matched.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Open to any institution", "Libraries", "Sweden", "Europe"]
        );
        assert_eq!(i.notes.len(), 3, "{:?}", i.notes);

        let body = seen.lock().unwrap()[0].1.clone();
        let props = &body["output_config"]["format"]["schema"]["properties"];
        assert_eq!(props["countries"]["items"]["enum"], json!(["DE", "SE"]));
        assert!(props["regions"]["items"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("brics")));
        let system = body["system"][0]["text"].as_str().unwrap();
        assert!(
            system.contains("- eu: the European Union; ") && system.contains("as of 2020-02-01")
        );
    }

    #[test]
    fn an_empty_vocabulary_still_makes_a_valid_list_schema() {
        assert_eq!(
            enum_array(vec![]),
            json!({ "type": "array", "items": { "type": "null" } })
        );
        assert_eq!(enum_array(vec!["A".into()])["items"]["enum"], json!(["A"]));
    }

    #[tokio::test]
    async fn refusal_falls_back_to_rules() {
        let (base, _) = stub(message("refusal", "")).await;
        let i = interpret(
            Some(&reader(base)),
            &catalog(),
            "services in Sweden that request email",
            "auto",
        )
        .await;
        assert_eq!(i.engine, "rules");
        assert_eq!(i.params.country.as_deref(), Some("SE"));
        assert!(i.notes.iter().any(|n| n.contains("built-in rules")));
    }

    #[tokio::test]
    async fn short_queries_skip_the_model() {
        let (base, seen) = stub(message("end_turn", "{}")).await;
        let i = interpret(Some(&reader(base)), &catalog(), "overleaf", "auto").await;
        assert_eq!(i.engine, "rules");
        assert!(seen.lock().unwrap().is_empty());
    }
}
