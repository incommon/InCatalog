// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! In-memory query engine over a [`Snapshot`].
//!
//! The whole catalog is ~10k services, so every query is a linear scan over
//! precomputed lowercase search text. That keeps the API stateless, fast enough on a
//! cold Lambda, and free of an external search dependency.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::categories::{self, CategoryDef};
use crate::curation::provider_name;
use crate::federations::normalize_reg_auth;
use crate::model::{
    pick, Availability, Curated, Federation, Service, Snapshot, Source, SourceInfo,
};
use crate::nl::{Interpretation, Vocab};
use crate::INCOMMON_REGISTRAR;

pub const MAX_PER_PAGE: usize = 100;

#[derive(Debug, Clone, Default, Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
pub struct ServiceQuery {
    /// Free text; every word must appear in the name, description, keywords,
    /// organization, entityID, federation, or curated tags.
    pub q: Option<String>,
    /// eduGAIN federation code (e.g. `INCOMMON`) or registration authority URI.
    pub federation: Option<String>,
    /// ISO 3166 country code of the registering federation.
    pub country: Option<String>,
    /// Comma-separated category keys; a service must carry all of them.
    pub category: Option<String>,
    /// One of the availability keys, e.g. `incommon-and-edugain`.
    pub availability: Option<String>,
    /// Requested attribute, by `Name` or `FriendlyName` (case-insensitive).
    pub attribute: Option<String>,
    /// Provider ID from `/v1/providers`.
    pub provider: Option<String>,
    /// `incommon` or `edugain`: which aggregate the service must appear in.
    pub source: Option<String>,
    /// Curated provider type, e.g. `commercial`.
    pub provider_type: Option<String>,
    /// Preferred language for names and descriptions (default `en`).
    pub lang: Option<String>,
    /// `relevance` (default when `q` is set), `name`, or `provider`.
    pub sort: Option<String>,
    pub page: Option<usize>,
    /// Page size, at most 100.
    pub per_page: Option<usize>,
    /// Set to `false` to skip facet counts.
    pub facets: Option<bool>,
    /// `all` (default): every word in `q` must appear. `any`: at least half must.
    /// `some`: at least one must. Results rank by how many words match, and the
    /// exact phrase ranks highest. Words match the start of a word ("journal"
    /// finds "journals", "art" does not find "start").
    #[serde(rename = "match")]
    #[param(rename = "match")]
    pub match_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ServiceSummary {
    pub id: String,
    pub entity_id: String,
    pub name: String,
    /// The metadata's description, or the generated summary when there is none
    /// (see `description_source`).
    pub description: Option<String>,
    /// `metadata` or `generated`.
    pub description_source: Option<String>,
    /// From the generated summary: `production`, `test`, `staging`,
    /// `development`, or `unknown`.
    pub environment: Option<String>,
    pub organization: Option<String>,
    pub provider_id: String,
    /// An `https:` or `data:image/` logo URL; plain `http:` logos are omitted.
    pub logo: Option<String>,
    /// `metadata` (the service's own `mdui:Logo`) or `website` (found on the
    /// organization's site because no metadata logo works).
    pub logo_source: Option<String>,
    pub categories: Vec<String>,
    pub availability: Option<Availability>,
    pub registrar: Option<String>,
    pub federation: Option<String>,
    pub federation_name: Option<String>,
    pub countries: Vec<String>,
    pub sources: Vec<Source>,
    pub curated: Option<Curated>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FacetValue {
    pub value: String,
    pub label: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ServicePage {
    pub total: usize,
    pub page: usize,
    pub per_page: usize,
    pub items: Vec<ServiceSummary>,
    /// Counts per facet value, each computed with every *other* active filter applied.
    pub facets: BTreeMap<String, Vec<FacetValue>>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Links {
    /// Live per-entity metadata from InCommon's MDQ service.
    pub mdq: Option<String>,
    /// REFEDS Metadata Explorer Tool: every federation that publishes this entity.
    pub met: String,
    /// eduGAIN technical site entity page.
    pub edugain: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProviderRef {
    pub id: String,
    pub name: String,
    pub service_count: usize,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ServiceDetail {
    pub name: String,
    pub service: Service,
    pub federation: Option<Federation>,
    pub provider: ProviderRef,
    pub links: Links,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub urls: Vec<String>,
    pub service_count: usize,
    pub registrars: Vec<String>,
    pub federations: Vec<String>,
    pub categories: BTreeMap<String, usize>,
    pub availability: BTreeMap<String, usize>,
    pub provider_type: Option<String>,
    #[serde(skip)]
    services: Vec<usize>,
    #[serde(skip)]
    search: String,
}

#[derive(Debug, Clone, Default, Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
pub struct ProviderQuery {
    pub q: Option<String>,
    /// `services` (default, most services first) or `name`.
    pub sort: Option<String>,
    pub page: Option<usize>,
    pub per_page: Option<usize>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProviderPage {
    pub total: usize,
    pub page: usize,
    pub per_page: usize,
    pub items: Vec<Provider>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProviderDetail {
    pub provider: Provider,
    pub services: Vec<ServiceSummary>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FederationView {
    #[serde(flatten)]
    pub federation: Federation,
    /// Whether eduGAIN's federation list describes this registrar.
    pub listed_in_edugain: bool,
    pub catalog_services: usize,
    pub catalog_identity_providers: usize,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CategoryView {
    #[serde(flatten)]
    pub def: CategoryDef,
    pub service_count: usize,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RawTag {
    pub attribute: String,
    pub value: String,
    pub service_count: usize,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CategoryReport {
    pub categories: Vec<CategoryView>,
    /// Every distinct tag value on a service, so nothing is hidden by the synonym table.
    pub raw: Vec<RawTag>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AttributeView {
    pub name: String,
    pub friendly_name: Option<String>,
    pub requested_by: usize,
    pub required_by: usize,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Counts {
    pub services: usize,
    pub providers: usize,
    pub federations: usize,
    pub identity_providers: usize,
    pub by_availability: BTreeMap<String, usize>,
    pub incommon_registered_services: usize,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Meta {
    pub schema_version: u32,
    pub generated_at: String,
    pub generator: String,
    pub sources: Vec<SourceInfo>,
    pub counts: Counts,
    pub warnings: Vec<String>,
    pub caveats: Vec<String>,
}

struct Entry {
    /// Normalized text (see [`normalize`]) for word-prefix matching.
    name_n: String,
    org_n: String,
    /// Generated search keywords, normalized, one per line-equivalent.
    keywords_n: String,
    /// Generated summary marks this as a test, staging, or development instance.
    non_production: bool,
    name: String,
    name_lc: String,
    org_lc: String,
    blob: String,
    provider: usize,
    countries: Vec<String>,
    reg_auth_norm: String,
    attrs_lc: Vec<String>,
}

pub struct Catalog {
    snapshot: Snapshot,
    vocab: Vocab,
    entries: Vec<Entry>,
    by_id: HashMap<String, usize>,
    by_entity_id: HashMap<String, usize>,
    providers: Vec<Provider>,
    provider_by_id: HashMap<String, usize>,
    fed_by_reg_auth: HashMap<String, usize>,
}

impl Catalog {
    pub fn new(snapshot: Snapshot) -> Self {
        let fed_by_reg_auth: HashMap<String, usize> = snapshot
            .federations
            .iter()
            .enumerate()
            .map(|(i, f)| (normalize_reg_auth(&f.registration_authority), i))
            .collect();

        let mut providers: Vec<Provider> = Vec::new();
        let mut provider_by_key: HashMap<String, usize> = HashMap::new();
        let mut entries = Vec::with_capacity(snapshot.services.len());

        for (i, s) in snapshot.services.iter().enumerate() {
            let fed = s
                .registrar()
                .and_then(|r| fed_by_reg_auth.get(&normalize_reg_auth(r)))
                .map(|&fi| &snapshot.federations[fi]);
            let pname = provider_name(s)
                .map(str::to_owned)
                .unwrap_or_else(|| host_of(&s.entity_id));
            let pkey = pname.trim().to_lowercase();
            let pidx = *provider_by_key.entry(pkey.clone()).or_insert_with(|| {
                providers.push(Provider {
                    id: crate::entity_hash(&pkey)[..16].to_owned(),
                    name: pname.clone(),
                    urls: Vec::new(),
                    service_count: 0,
                    registrars: Vec::new(),
                    federations: Vec::new(),
                    categories: BTreeMap::new(),
                    availability: BTreeMap::new(),
                    provider_type: None,
                    services: Vec::new(),
                    search: normalize(&pkey),
                });
                providers.len() - 1
            });
            let p = &mut providers[pidx];
            p.services.push(i);
            if !hidden(s) {
                p.service_count += 1;
                push_unique(&mut p.urls, pick(&s.organization.urls, None));
                push_unique(&mut p.registrars, s.registrar());
                push_unique(&mut p.federations, fed.map(|f| f.name.as_str()));
                for c in &s.categories {
                    *p.categories.entry(c.clone()).or_default() += 1;
                }
                if let Some(a) = s.availability {
                    *p.availability.entry(a.key().to_owned()).or_default() += 1;
                }
                if p.provider_type.is_none() {
                    p.provider_type = s.curated.as_ref().and_then(|c| c.provider_type.clone());
                }
            }

            let name = display_name(s, None);
            let mut blob = String::new();
            for v in s
                .display_names
                .values()
                .chain(s.descriptions.values())
                .chain(s.service_names.values())
                .chain(s.organization.display_names.values())
                .chain(s.organization.names.values())
                .chain(s.keywords.iter())
            {
                blob.push_str(v);
                blob.push('\n');
            }
            blob.push_str(&s.entity_id);
            blob.push('\n');
            if let Some(f) = fed {
                blob.push_str(&f.name);
                blob.push('\n');
                blob.push_str(&f.countries.join(" "));
            }
            if let Some(e) = &s.enrichment {
                blob.push('\n');
                blob.push_str(&e.summary);
                blob.push('\n');
                blob.push_str(&e.keywords.join(" | "));
            }
            if let Some(c) = &s.curated {
                blob.push('\n');
                blob.push_str(&c.tags.join(" "));
                if let Some(sum) = &c.summary {
                    blob.push('\n');
                    blob.push_str(sum);
                }
            }
            entries.push(Entry {
                name_n: normalize(&name),
                keywords_n: s
                    .enrichment
                    .as_ref()
                    .map(|e| e.keywords.iter().map(|k| normalize(k)).collect::<String>())
                    .unwrap_or_default(),
                non_production: s.enrichment.as_ref().is_some_and(|e| {
                    matches!(e.environment.as_str(), "test" | "staging" | "development")
                }),
                org_n: normalize(&pkey),
                name_lc: name.to_lowercase(),
                name,
                org_lc: pkey,
                blob: normalize(&blob),
                provider: pidx,
                countries: countries_for(s, fed),
                reg_auth_norm: s.registrar().map(normalize_reg_auth).unwrap_or_default(),
                attrs_lc: s
                    .requested_attributes
                    .iter()
                    .flat_map(|a| {
                        std::iter::once(a.name.to_lowercase())
                            .chain(a.friendly_name.as_ref().map(|f| f.to_lowercase()))
                    })
                    .collect(),
            });
        }

        let by_id = snapshot
            .services
            .iter()
            .enumerate()
            .map(|(i, s)| (s.id.clone(), i))
            .collect();
        let by_entity_id = snapshot
            .services
            .iter()
            .enumerate()
            .map(|(i, s)| (s.entity_id.clone(), i))
            .collect();
        let provider_by_id = providers
            .iter()
            .enumerate()
            .map(|(i, p)| (p.id.clone(), i))
            .collect();

        Self {
            vocab: Vocab::new(&snapshot.services, &snapshot.federations),
            snapshot,
            entries,
            by_id,
            by_entity_id,
            providers,
            provider_by_id,
            fed_by_reg_auth,
        }
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// The filter vocabulary natural-language queries are read into.
    pub fn vocab(&self) -> &Vocab {
        &self.vocab
    }

    /// Read a natural-language query with the built-in rules.
    pub fn interpret(&self, query: &str) -> Interpretation {
        self.vocab.interpret(query)
    }

    fn federation_of(&self, s: &Service) -> Option<&Federation> {
        s.registrar()
            .and_then(|r| self.fed_by_reg_auth.get(&normalize_reg_auth(r)))
            .map(|&i| &self.snapshot.federations[i])
    }

    pub fn summary(&self, i: usize, lang: Option<&str>) -> ServiceSummary {
        let s = &self.snapshot.services[i];
        let e = &self.entries[i];
        let fed = self.federation_of(s);
        ServiceSummary {
            id: s.id.clone(),
            entity_id: s.entity_id.clone(),
            name: display_name(s, lang),
            description: description(s, lang).map(|(d, _)| truncate(d, 300)),
            description_source: description(s, lang).map(|(_, src)| src.to_owned()),
            environment: s.enrichment.as_ref().map(|e| e.environment.clone()),
            organization: pick(&s.organization.display_names, lang)
                .or_else(|| pick(&s.organization.names, lang))
                .map(str::to_owned),
            provider_id: self.providers[e.provider].id.clone(),
            logo: best_logo(s).map(|(u, _)| u),
            logo_source: best_logo(s).map(|(_, src)| src.to_owned()),
            categories: s.categories.clone(),
            availability: s.availability,
            registrar: s.registrar().map(str::to_owned),
            federation: fed.map(|f| f.code.clone()),
            federation_name: fed.map(|f| f.name.clone()),
            countries: e.countries.clone(),
            sources: s.sources.clone(),
            curated: s.curated.clone(),
        }
    }

    pub fn service_by_id(&self, id: &str) -> Option<ServiceDetail> {
        let id = id.trim().to_lowercase();
        let id = id.strip_prefix("{sha1}").unwrap_or(&id);
        self.by_id.get(id).map(|&i| self.detail(i))
    }

    pub fn service_by_entity_id(&self, entity_id: &str) -> Option<ServiceDetail> {
        self.by_entity_id.get(entity_id).map(|&i| self.detail(i))
    }

    fn detail(&self, i: usize) -> ServiceDetail {
        let s = &self.snapshot.services[i];
        let p = &self.providers[self.entries[i].provider];
        ServiceDetail {
            name: self.entries[i].name.clone(),
            service: s.clone(),
            federation: self.federation_of(s).cloned(),
            provider: ProviderRef {
                id: p.id.clone(),
                name: p.name.clone(),
                service_count: p.service_count,
            },
            links: links(s),
        }
    }

    pub fn search(&self, q: &ServiceQuery) -> ServicePage {
        let mut f = Filters::new(q);
        if !f.tokens.is_empty() && q.sort.as_deref().unwrap_or("relevance") == "relevance" {
            f.weigh(&self.entries);
        }
        let lang = q.lang.as_deref();
        let text_ok: Vec<bool> = self.entries.iter().map(|e| f.text_match(e)).collect();
        let base = |i: usize| text_ok[i] && !hidden(&self.snapshot.services[i]);

        let mut hits: Vec<(usize, u32)> = (0..self.entries.len())
            .filter(|&i| base(i) && f.matches(self, i, None))
            .map(|i| (i, f.score(&self.entries[i])))
            .collect();

        let sort = q.sort.as_deref().unwrap_or(if f.tokens.is_empty() {
            "name"
        } else {
            "relevance"
        });
        match sort {
            "provider" => hits.sort_by(|a, b| {
                (&self.entries[a.0].org_lc, &self.entries[a.0].name_lc)
                    .cmp(&(&self.entries[b.0].org_lc, &self.entries[b.0].name_lc))
            }),
            "relevance" => hits.sort_by(|a, b| {
                b.1.cmp(&a.1)
                    .then_with(|| self.entries[a.0].name_lc.cmp(&self.entries[b.0].name_lc))
            }),
            _ => hits.sort_by(|a, b| self.entries[a.0].name_lc.cmp(&self.entries[b.0].name_lc)),
        }

        let (page, per_page) = paging(q.page, q.per_page);
        let items = hits
            .iter()
            .skip((page - 1) * per_page)
            .take(per_page)
            .map(|&(i, _)| self.summary(i, lang))
            .collect();

        let facets = if q.facets == Some(false) {
            BTreeMap::new()
        } else {
            self.facets(&f, &base)
        };
        ServicePage {
            total: hits.len(),
            page,
            per_page,
            items,
            facets,
        }
    }

    fn facets(
        &self,
        f: &Filters,
        base: &dyn Fn(usize) -> bool,
    ) -> BTreeMap<String, Vec<FacetValue>> {
        let mut out = BTreeMap::new();
        for dim in Dim::ALL {
            let mut counts: HashMap<String, (String, usize)> = HashMap::new();
            for i in 0..self.entries.len() {
                if !base(i) || !f.matches(self, i, Some(dim)) {
                    continue;
                }
                let s = &self.snapshot.services[i];
                let mut bump = |k: &str, label: &str| {
                    counts
                        .entry(k.to_owned())
                        .or_insert_with(|| (label.to_owned(), 0))
                        .1 += 1;
                };
                match dim {
                    Dim::Federation => {
                        let r = s.registrar().unwrap_or("");
                        let label = self.federation_of(s).map(|f| f.name.as_str()).unwrap_or(r);
                        bump(r, label);
                    }
                    Dim::Country => {
                        for c in &self.entries[i].countries {
                            bump(c, c);
                        }
                    }
                    Dim::Category => {
                        for c in &s.categories {
                            bump(c, categories::get(c).map(|d| d.label).unwrap_or(c));
                        }
                    }
                    Dim::Availability => {
                        if let Some(a) = s.availability {
                            bump(a.key(), a.label());
                        }
                    }
                    Dim::Attribute => {
                        for a in &s.requested_attributes {
                            bump(&a.name, a.friendly_name.as_deref().unwrap_or(&a.name));
                        }
                    }
                    Dim::Source => {
                        for src in &s.sources {
                            let k = source_key(*src);
                            bump(
                                k,
                                if *src == Source::Incommon {
                                    "InCommon aggregate"
                                } else {
                                    "eduGAIN aggregate"
                                },
                            );
                        }
                    }
                    Dim::ProviderType => {
                        if let Some(t) = s.curated.as_ref().and_then(|c| c.provider_type.as_deref())
                        {
                            bump(&t.to_lowercase(), t);
                        }
                    }
                }
            }
            let mut values: Vec<FacetValue> = counts
                .into_iter()
                .map(|(value, (label, count))| FacetValue {
                    value,
                    label,
                    count,
                })
                .collect();
            values.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
            if dim == Dim::Attribute {
                values.truncate(40);
            }
            out.insert(dim.key().to_owned(), values);
        }
        out
    }

    pub fn providers(&self, q: &ProviderQuery) -> ProviderPage {
        let tokens = tokens(q.q.as_deref());
        let mut hits: Vec<&Provider> = self
            .providers
            .iter()
            .filter(|p| p.service_count > 0 && tokens.iter().all(|t| p.search.contains(t.as_str())))
            .collect();
        match q.sort.as_deref() {
            Some("name") => hits.sort_by(|a, b| a.search.cmp(&b.search)),
            _ => hits.sort_by(|a, b| {
                b.service_count
                    .cmp(&a.service_count)
                    .then_with(|| a.search.cmp(&b.search))
            }),
        }
        let (page, per_page) = paging(q.page, q.per_page);
        ProviderPage {
            total: hits.len(),
            page,
            per_page,
            items: hits
                .into_iter()
                .skip((page - 1) * per_page)
                .take(per_page)
                .cloned()
                .collect(),
        }
    }

    pub fn provider(&self, id: &str, lang: Option<&str>) -> Option<ProviderDetail> {
        let p = &self.providers[*self.provider_by_id.get(id)?];
        let mut services: Vec<ServiceSummary> = p
            .services
            .iter()
            .filter(|&&i| !hidden(&self.snapshot.services[i]))
            .map(|&i| self.summary(i, lang))
            .collect();
        services.sort_by_key(|s| s.name.to_lowercase());
        Some(ProviderDetail {
            provider: p.clone(),
            services,
        })
    }

    pub fn federations(&self) -> Vec<FederationView> {
        let mut services: HashMap<String, usize> = HashMap::new();
        for s in &self.snapshot.services {
            if !hidden(s) {
                *services
                    .entry(s.registrar().map(normalize_reg_auth).unwrap_or_default())
                    .or_default() += 1;
            }
        }
        let mut idps: HashMap<String, usize> = HashMap::new();
        for (r, n) in &self.snapshot.idp_counts {
            *idps.entry(normalize_reg_auth(r)).or_default() += n;
        }
        let mut out: Vec<FederationView> = self
            .snapshot
            .federations
            .iter()
            .map(|f| {
                let k = normalize_reg_auth(&f.registration_authority);
                FederationView {
                    federation: f.clone(),
                    listed_in_edugain: true,
                    catalog_services: services.remove(&k).unwrap_or(0),
                    catalog_identity_providers: idps.remove(&k).unwrap_or(0),
                }
            })
            .collect();
        // Registrars seen in metadata that eduGAIN's list does not describe.
        let mut leftovers: Vec<String> = services.keys().chain(idps.keys()).cloned().collect();
        leftovers.sort();
        leftovers.dedup();
        for k in leftovers {
            out.push(FederationView {
                federation: Federation {
                    name: if k.is_empty() {
                        "(no registration authority)".into()
                    } else {
                        k.clone()
                    },
                    registration_authority: k.clone(),
                    status: "unknown".into(),
                    ..Default::default()
                },
                listed_in_edugain: false,
                catalog_services: services.get(&k).copied().unwrap_or(0),
                catalog_identity_providers: idps.get(&k).copied().unwrap_or(0),
            });
        }
        out
    }

    pub fn categories(&self) -> CategoryReport {
        let visible = || self.snapshot.services.iter().filter(|s| !hidden(s));
        let categories = categories::CATEGORIES
            .iter()
            .map(|d| CategoryView {
                def: d.clone(),
                service_count: visible()
                    .filter(|s| s.categories.iter().any(|c| c == d.key))
                    .count(),
            })
            .collect();
        let mut raw: BTreeMap<(&str, &str), usize> = BTreeMap::new();
        for s in visible() {
            for v in &s.entity_categories {
                *raw.entry(("entity-category", v)).or_default() += 1;
            }
            for v in &s.entity_category_support {
                *raw.entry(("entity-category-support", v)).or_default() += 1;
            }
            for v in &s.assurance_certifications {
                *raw.entry(("assurance-certification", v)).or_default() += 1;
            }
        }
        let mut raw: Vec<RawTag> = raw
            .into_iter()
            .map(|((a, v), n)| RawTag {
                attribute: a.into(),
                value: v.into(),
                service_count: n,
            })
            .collect();
        raw.sort_by_key(|r| std::cmp::Reverse(r.service_count));
        CategoryReport { categories, raw }
    }

    pub fn attributes(&self) -> Vec<AttributeView> {
        let mut m: HashMap<&str, (HashMap<&str, usize>, usize, usize)> = HashMap::new();
        for s in self.snapshot.services.iter().filter(|s| !hidden(s)) {
            for a in &s.requested_attributes {
                let e = m.entry(&a.name).or_default();
                if let Some(f) = &a.friendly_name {
                    *e.0.entry(f).or_default() += 1;
                }
                e.1 += 1;
                if a.required {
                    e.2 += 1;
                }
            }
        }
        let mut out: Vec<AttributeView> = m
            .into_iter()
            .map(|(name, (friendly, n, req))| AttributeView {
                name: name.to_owned(),
                friendly_name: friendly
                    .into_iter()
                    .max_by_key(|(_, c)| *c)
                    .map(|(f, _)| f.to_owned()),
                requested_by: n,
                required_by: req,
            })
            .collect();
        out.sort_by(|a, b| {
            b.requested_by
                .cmp(&a.requested_by)
                .then_with(|| a.name.cmp(&b.name))
        });
        out
    }

    pub fn meta(&self) -> Meta {
        let visible: Vec<&Service> = self
            .snapshot
            .services
            .iter()
            .filter(|s| !hidden(s))
            .collect();
        let mut by_availability = BTreeMap::new();
        for s in &visible {
            if let Some(a) = s.availability {
                *by_availability.entry(a.key().to_owned()).or_default() += 1;
            }
        }
        let mut caveats = vec![
            "Presence in federation metadata describes configuration. It does not establish that a service is suitable, contracted, or currently operating.".to_owned(),
            "Provider grouping uses the organization name each registrant chose. It does not resolve parent/subsidiary or system/campus relationships.".to_owned(),
            "InCommon's eduGAIN import is filtered, so absence from InCommon's aggregate does not mean absence from eduGAIN.".to_owned(),
            "Entity categories record what was asserted or assigned in metadata; they do not guarantee attribute release by any particular identity provider.".to_owned(),
            "Services that federate only with OpenID Connect do not appear in SAML metadata.".to_owned(),
        ];
        if self.snapshot.sources.iter().any(|s| !s.signature_verified) {
            caveats.push("Aggregates were fetched over TLS from the federation operators, but their XML signatures were not verified for this snapshot. Do not use this catalog as a trust source for SAML configuration; consume signed metadata directly.".to_owned());
        }
        Meta {
            schema_version: self.snapshot.schema_version,
            generated_at: self.snapshot.generated_at.clone(),
            generator: self.snapshot.generator.clone(),
            sources: self.snapshot.sources.clone(),
            counts: Counts {
                services: visible.len(),
                providers: self
                    .providers
                    .iter()
                    .filter(|p| p.service_count > 0)
                    .count(),
                federations: self.snapshot.federations.len(),
                identity_providers: self.snapshot.idp_counts.values().sum(),
                incommon_registered_services: visible
                    .iter()
                    .filter(|s| s.registrar() == Some(INCOMMON_REGISTRAR))
                    .count(),
                by_availability,
            },
            warnings: self.snapshot.warnings.clone(),
            caveats,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dim {
    Federation,
    Country,
    Category,
    Availability,
    Attribute,
    Source,
    ProviderType,
}

impl Dim {
    const ALL: [Dim; 7] = [
        Dim::Federation,
        Dim::Country,
        Dim::Category,
        Dim::Availability,
        Dim::Attribute,
        Dim::Source,
        Dim::ProviderType,
    ];

    fn key(self) -> &'static str {
        match self {
            Dim::Federation => "federation",
            Dim::Country => "country",
            Dim::Category => "category",
            Dim::Availability => "availability",
            Dim::Attribute => "attribute",
            Dim::Source => "source",
            Dim::ProviderType => "provider_type",
        }
    }
}

struct Filters {
    /// Query words, stemmed, each with a leading space so `contains` means "a word
    /// starts with this".
    tokens: Vec<String>,
    /// The whole query, normalized, for the exact-phrase bonus.
    phrase: String,
    /// How many tokens must match.
    need: usize,
    /// Per-token weight: rarer words count for more (see [`Filters::weigh`]).
    weights: Vec<u32>,
    federation: Option<String>,
    country: Option<String>,
    categories: Vec<String>,
    /// `Some(None)` means an unrecognized availability key: match nothing.
    availability: Option<Option<Availability>>,
    attribute: Option<String>,
    provider: Option<String>,
    source: Option<String>,
    provider_type: Option<String>,
}

impl Filters {
    fn new(q: &ServiceQuery) -> Self {
        let nonempty = |o: &Option<String>| {
            o.as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        let tokens = tokens(q.q.as_deref());
        let need = match q.match_mode.as_deref() {
            Some("some") => 1,
            Some("any") => tokens.len().div_ceil(2),
            _ => tokens.len(),
        };
        Self {
            phrase: normalize(q.q.as_deref().unwrap_or("")),
            need: need.min(tokens.len()),
            weights: vec![10; tokens.len()],
            tokens,
            federation: nonempty(&q.federation),
            country: nonempty(&q.country).map(|c| c.to_uppercase()),
            categories: nonempty(&q.category)
                .map(|c| {
                    c.split(',')
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            availability: nonempty(&q.availability).map(|a| Availability::from_key(&a)),
            attribute: nonempty(&q.attribute).map(|a| a.to_lowercase()),
            provider: nonempty(&q.provider),
            source: nonempty(&q.source).map(|s| s.to_lowercase()),
            provider_type: nonempty(&q.provider_type).map(|s| s.to_lowercase()),
        }
    }

    /// Inverse document frequency: in "survey tools", "survey" is rare and decides
    /// relevance, while "tools" appears in hundreds of services and should not.
    fn weigh(&mut self, entries: &[Entry]) {
        let n = entries.len() as f64;
        self.weights = self
            .tokens
            .iter()
            .map(|t| {
                let df = entries
                    .iter()
                    .filter(|e| e.blob.contains(t.as_str()))
                    .count() as f64;
                (((n + 1.0) / (df + 1.0)).ln() * 10.0) as u32 + 1
            })
            .collect();
    }

    fn text_match(&self, e: &Entry) -> bool {
        self.tokens.is_empty()
            || self
                .tokens
                .iter()
                .filter(|t| e.blob.contains(t.as_str()))
                .count()
                >= self.need
    }

    fn matches(&self, c: &Catalog, i: usize, except: Option<Dim>) -> bool {
        let s = &c.snapshot.services[i];
        let e = &c.entries[i];
        let on = |d: Dim| except != Some(d);
        if on(Dim::Federation) {
            if let Some(f) = &self.federation {
                let by_code = s
                    .federation
                    .as_deref()
                    .is_some_and(|code| code.eq_ignore_ascii_case(f));
                if !by_code && e.reg_auth_norm != normalize_reg_auth(f) {
                    return false;
                }
            }
        }
        if on(Dim::Country) {
            if let Some(cc) = &self.country {
                if !e.countries.contains(cc) {
                    return false;
                }
            }
        }
        if on(Dim::Category) && !self.categories.iter().all(|k| s.categories.contains(k)) {
            return false;
        }
        if on(Dim::Availability) {
            if let Some(a) = &self.availability {
                if s.availability != *a || a.is_none() {
                    return false;
                }
            }
        }
        if on(Dim::Attribute) {
            if let Some(a) = &self.attribute {
                if !e.attrs_lc.contains(a) {
                    return false;
                }
            }
        }
        if let Some(p) = &self.provider {
            if c.providers[e.provider].id != *p {
                return false;
            }
        }
        if on(Dim::Source) {
            if let Some(src) = &self.source {
                if !s.sources.iter().any(|x| source_key(*x) == src) {
                    return false;
                }
            }
        }
        if on(Dim::ProviderType) {
            if let Some(t) = &self.provider_type {
                let have = s
                    .curated
                    .as_ref()
                    .and_then(|c| c.provider_type.as_deref())
                    .map(str::to_lowercase);
                if have.as_deref() != Some(t.as_str()) {
                    return false;
                }
            }
        }
        true
    }

    fn score(&self, e: &Entry) -> u32 {
        if self.tokens.is_empty() {
            return 0;
        }
        let mut score = 1;
        let p = self.phrase.as_str();
        if e.name_n == p {
            score += 400;
        } else if e.name_n.starts_with(p) {
            score += 250;
        } else if e.name_n.contains(p) {
            score += 150;
        } else if e.keywords_n.contains(p) {
            // The whole query is one of the generated keywords ("lms", "survey").
            score += 120;
        } else if e.blob.contains(p) {
            score += 100;
        }
        for (t, w) in self.tokens.iter().zip(&self.weights) {
            if e.blob.contains(t.as_str()) {
                score += w * 2;
            }
            if e.name_n.contains(t.as_str()) {
                score += w;
            }
            if e.org_n.contains(t.as_str()) {
                score += w / 2;
            }
            if e.keywords_n.contains(t.as_str()) {
                score += w;
            }
        }
        // Test and staging copies sit behind the live service they mirror.
        if e.non_production {
            score = score / 3 + 1;
        }
        score
    }
}

/// Words too common to require; they still count toward the exact-phrase bonus.
const FILLER: &[&str] = &[
    "a", "an", "the", "of", "for", "and", "or", "to", "in", "on", "at", "by", "with", "from",
];

fn tokens(q: Option<&str>) -> Vec<String> {
    let words: Vec<String> = normalize(q.unwrap_or(""))
        .split_whitespace()
        .map(stem)
        .collect();
    let content: Vec<&String> = words
        .iter()
        .filter(|w| !FILLER.contains(&w.as_str()))
        .collect();
    // A query made only of filler ("the who") is searched as typed.
    let keep: Vec<&String> = if content.is_empty() {
        words.iter().collect()
    } else {
        content
    };
    let mut out: Vec<String> = Vec::new();
    for w in keep {
        let t = format!(" {w}");
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// Lowercase, with hyphens and apostrophes inside words removed ("e-book" and
/// "ebook" match each other) and all other punctuation turned into single spaces.
/// The result starts and ends with a space, so `contains(" word")` tests for a
/// word beginning and `contains(" two words ")` for a whole phrase.
pub(crate) fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push(' ');
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if (c == '-' || c == '\'' || c == '’')
            && i > 0
            && i + 1 < chars.len()
            && chars[i - 1].is_alphanumeric()
            && chars[i + 1].is_alphanumeric()
        {
            // joined
        } else if !out.ends_with(' ') {
            out.push(' ');
        }
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

/// Just enough stemming that "journals" finds "journal" and "libraries" finds
/// "library". Tokens match word beginnings, so a shorter stem only widens.
fn stem(t: &str) -> String {
    if t.len() > 4 && t.ends_with("ies") {
        t[..t.len() - 3].to_owned()
    } else if t.len() > 3 && t.ends_with('s') && !t.ends_with("ss") && !t.ends_with("us") {
        t[..t.len() - 1].to_owned()
    } else {
        t.to_owned()
    }
}

fn paging(page: Option<usize>, per_page: Option<usize>) -> (usize, usize) {
    (
        page.unwrap_or(1).max(1),
        per_page.unwrap_or(24).clamp(1, MAX_PER_PAGE),
    )
}

fn hidden(s: &Service) -> bool {
    s.curated.as_ref().is_some_and(|c| c.hidden)
}

fn source_key(s: Source) -> &'static str {
    match s {
        Source::Incommon => "incommon",
        Source::Edugain => "edugain",
    }
}

fn push_unique(v: &mut Vec<String>, item: Option<&str>) {
    if let Some(item) = item {
        if !v.iter().any(|x| x == item) {
            v.push(item.to_owned());
        }
    }
}

fn countries_for(s: &Service, fed: Option<&Federation>) -> Vec<String> {
    match fed {
        Some(f) => f.country_codes.clone(),
        // InCommon is listed by eduGAIN with its country; this covers an empty list.
        None if s.registrar() == Some(INCOMMON_REGISTRAR) => vec!["US".into()],
        None => Vec::new(),
    }
}

/// The registrant's description in the requested language, else the generated
/// summary (always English).
fn description<'a>(s: &'a Service, lang: Option<&str>) -> Option<(&'a str, &'static str)> {
    pick(&s.descriptions, lang)
        .or_else(|| pick(&s.service_descriptions, lang))
        .map(|d| (d, "metadata"))
        .or_else(|| {
            s.enrichment
                .as_ref()
                .map(|e| e.summary.as_str())
                .filter(|t| !t.is_empty())
                .map(|t| (t, "generated"))
        })
}

/// MDUI DisplayName, then AttributeConsumingService name, then organization, then entityID.
pub fn display_name(s: &Service, lang: Option<&str>) -> String {
    pick(&s.display_names, lang)
        .or_else(|| pick(&s.service_names, lang))
        .or_else(|| pick(&s.organization.display_names, lang))
        .or_else(|| pick(&s.organization.names, lang))
        .unwrap_or(&s.entity_id)
        .to_owned()
}

/// The logo to show, and where it came from. A working metadata logo always wins:
/// `https:` or inline, not marked broken by ingest, English or untagged first, then
/// the largest. Otherwise the logo ingest found on the organization's website.
pub fn best_logo(s: &Service) -> Option<(String, &'static str)> {
    let from_metadata = s
        .logos
        .iter()
        .filter(|l| {
            !l.broken && (l.url.starts_with("https://") || l.url.starts_with("data:image/"))
        })
        .max_by_key(|l| {
            let lang_ok = matches!(l.lang.as_deref(), None | Some("en"));
            (
                lang_ok,
                l.width.unwrap_or(0).min(512) * l.height.unwrap_or(0).min(512),
            )
        })
        .map(|l| (l.url.clone(), "metadata"));
    from_metadata.or_else(|| {
        s.discovered_logo
            .as_ref()
            .map(|d| (d.url.clone(), "website"))
    })
}

fn links(s: &Service) -> Links {
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    let enc = utf8_percent_encode(&s.entity_id, NON_ALPHANUMERIC).to_string();
    Links {
        mdq: s
            .sources
            .contains(&Source::Incommon)
            .then(|| format!("https://mdq.incommon.org/entities/%7Bsha1%7D{}", s.id)),
        met: format!("https://met.refeds.org/met/entity/{enc}/"),
        edugain: s.sources.contains(&Source::Edugain).then(|| {
            format!("https://technical.edugain.org/api?action=show_entity_details&format=html&opt=standalone&e_id={enc}")
        }),
    }
}

fn host_of(entity_id: &str) -> String {
    entity_id
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', ':']).next().unwrap_or(rest).to_owned())
        .unwrap_or_else(|| entity_id.to_owned())
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{}…", cut.trim_end())
}
