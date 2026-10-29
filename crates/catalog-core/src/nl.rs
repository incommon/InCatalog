// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Natural-language queries, read into the catalog's structured filters.
//!
//! "Research & Scholarship services in Sweden that request email" becomes
//! `category=research-and-scholarship&country=SE&attribute=urn:oid:…3` with no
//! keywords left over. The rules here are deterministic, need no network, and only
//! ever produce values that exist in the loaded catalog. The API can layer a language
//! model on top (see `catalog-api`), but its output goes through [`Vocab::validate`]
//! so it is held to the same vocabulary.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::categories::CATEGORIES;
use crate::environment::Environment;
use crate::model::{Availability, Federation, Service};
use crate::regions::{self, REGIONS};
use crate::search::split_list;
use crate::sector::Sector;
use crate::sharing::Sharing;

/// The filters a query was read as. Field names and values match `/v1/services`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct InterpretedParams {
    /// Words left over for free-text matching.
    pub q: Option<String>,
    /// Comma-separated federation codes; a service registered by any of them matches.
    pub federation: Option<String>,
    /// Comma-separated ISO country codes; a service registered in any of them matches.
    pub country: Option<String>,
    /// Comma-separated region keys (see `/v1/regions`); each stands for its countries.
    pub region: Option<String>,
    /// Comma-separated environments; a service in any of them matches.
    pub environment: Option<String>,
    /// Comma-separated sharing labels (`multilateral`, `bilateral`, `unknown`).
    pub sharing: Option<String>,
    /// Comma-separated sectors of the organization that runs the service.
    pub sector: Option<String>,
    /// Comma-separated category keys.
    pub category: Option<String>,
    pub availability: Option<String>,
    pub attribute: Option<String>,
    pub source: Option<String>,
    pub provider_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MatchedTerm {
    /// The words in the query that produced this filter.
    pub text: String,
    pub field: String,
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Interpretation {
    pub query: String,
    /// `rules`, or the model that read the query.
    pub engine: String,
    pub params: InterpretedParams,
    pub matched: Vec<MatchedTerm>,
    /// Anything the reader dropped or could not apply, in plain language.
    pub notes: Vec<String>,
    /// One-sentence reading of the query, when a language model produced it.
    pub explanation: Option<String>,
}

impl Interpretation {
    /// True when the query produced at least one filter beyond free text.
    pub fn has_filters(&self) -> bool {
        let p = &self.params;
        [
            &p.federation,
            &p.country,
            &p.region,
            &p.environment,
            &p.sharing,
            &p.sector,
            &p.category,
            &p.availability,
            &p.attribute,
            &p.source,
            &p.provider_type,
        ]
        .iter()
        .any(|v| v.is_some())
    }
}

/// Everything a query may be read into, taken from the loaded catalog.
#[derive(Debug, Clone, Default)]
pub struct Vocab {
    /// Federation code → (name, country codes).
    pub federations: BTreeMap<String, (String, Vec<String>)>,
    /// ISO country code → country name.
    pub countries: BTreeMap<String, String>,
    /// Requested attribute Name → friendly name, most requested first.
    pub attributes: Vec<(String, Option<String>)>,
    /// Curated provider types in use (lowercase → as written).
    pub provider_types: BTreeMap<String, String>,
    phrases: Vec<Phrase>,
}

#[derive(Debug, Clone)]
struct Phrase {
    words: Vec<String>,
    field: Field,
    value: String,
    label: String,
    /// Only applies after a verb like "request" or "need" ("email services" is a
    /// subject, "services that request email" is an attribute).
    needs_cue: bool,
    /// A short federation code that is also an everyday word (LEAF, RASH, SIR, CAFe):
    /// only applies when typed in capitals or next to "federation".
    shouty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Field {
    // Order is match priority when two phrases of equal length compete.
    Availability,
    Source,
    Environment,
    Sharing,
    Sector,
    Category,
    Federation,
    Region,
    Country,
    Attribute,
    ProviderType,
}

impl Field {
    fn key(self) -> &'static str {
        match self {
            Field::Availability => "availability",
            Field::Source => "source",
            Field::Environment => "environment",
            Field::Sharing => "sharing",
            Field::Sector => "sector",
            Field::Category => "category",
            Field::Federation => "federation",
            Field::Country => "country",
            Field::Region => "region",
            Field::Attribute => "attribute",
            Field::ProviderType => "provider_type",
        }
    }
}

const CUES: &[&str] = &[
    "request",
    "requests",
    "requesting",
    "requested",
    "require",
    "requires",
    "requiring",
    "need",
    "needs",
    "needing",
    "ask",
    "asks",
    "asking",
    "want",
    "wants",
    "consume",
    "consumes",
    "release",
    "released",
    "releases",
    "receive",
    "receives",
    "get",
    "gets",
];

const STOPWORDS: &[&str] = &[
    "a",
    "an",
    "the",
    "and",
    "or",
    "of",
    "for",
    "to",
    "in",
    "on",
    "at",
    "by",
    "from",
    "with",
    "without",
    "that",
    "which",
    "who",
    "whose",
    "what",
    "where",
    "are",
    "is",
    "be",
    "do",
    "does",
    "can",
    "could",
    "i",
    "me",
    "my",
    "we",
    "our",
    "us",
    "you",
    "show",
    "find",
    "list",
    "search",
    "give",
    "any",
    "all",
    "some",
    "there",
    "please",
    "looking",
    "look",
    "via",
    "through",
    "into",
    "using",
    "use",
    "used",
    "log",
    "login",
    "sign",
    "available",
    "accessible",
    "offered",
    "offer",
    "offers",
    "registered",
    "service",
    "services",
    "sp",
    "sps",
    "app",
    "apps",
    "application",
    "applications",
    "site",
    "sites",
    "website",
    "websites",
    "it",
    "its",
    "their",
    "them",
    "this",
    "these",
    "those",
    "also",
    "only",
    "both",
    "like",
    "such",
    "as",
    "federated",
    "federation",
    "federations",
    "provider",
    "providers",
    "support",
    "supports",
    "supporting",
    "category",
    "categories",
    "attribute",
    "attributes",
    "tagged",
    "tag",
    "users",
    "user",
    "people",
    "someone",
    "something",
    "anything",
    "things",
    "thing",
    "stuff",
    // Who a service is for rarely appears in its metadata.
    "researchers",
    "researcher",
    "scholars",
    "scientists",
    "faculty",
    "academics",
];

/// Short words kept when they join two content words, so "web of science" stays a
/// phrase instead of becoming "web science".
const LINKS: &[&str] = &[
    "of", "for", "and", "the", "in", "on", "to", "with", "&", "de", "la",
];

/// Provider-type words, which only become a filter when curation supplies the types.
const PROVIDER_TYPE_WORDS: &[&str] = &[
    "commercial",
    "vendor",
    "vendors",
    "non-commercial",
    "noncommercial",
    "nonprofit",
    "non-profit",
];

/// Natural-language spellings for common attributes, keyed by friendly name.
const ATTRIBUTE_ALIASES: &[(&str, &[&str], bool)] = &[
    // (friendly name, phrases, phrase is unambiguous without a cue)
    ("mail", &["email", "e-mail", "email address", "mail"], false),
    (
        "eduPersonPrincipalName",
        &["eppn", "principal name", "edupersonprincipalname"],
        true,
    ),
    (
        "eduPersonScopedAffiliation",
        &[
            "scoped affiliation",
            "affiliation",
            "edupersonscopedaffiliation",
        ],
        false,
    ),
    (
        "eduPersonAffiliation",
        &["unscoped affiliation", "edupersonaffiliation"],
        true,
    ),
    (
        "eduPersonEntitlement",
        &["entitlement", "entitlements", "edupersonentitlement"],
        false,
    ),
    (
        "eduPersonTargetedID",
        &["targeted id", "persistent id", "edupersontargetedid"],
        true,
    ),
    (
        "eduPersonOrcid",
        &["orcid", "orcid id", "edupersonorcid"],
        true,
    ),
    (
        "givenName",
        &["first name", "given name", "givenname"],
        false,
    ),
    ("sn", &["last name", "surname", "family name"], false),
    (
        "displayName",
        &["display name", "full name", "displayname"],
        false,
    ),
    ("cn", &["common name"], false),
    (
        "schacHomeOrganization",
        &["home organization", "schachomeorganization"],
        true,
    ),
    ("pairwise-id", &["pairwise id", "pairwise-id"], true),
    ("subject-id", &["subject id", "subject-id"], true),
];

/// Adjectives and alternate names that the eduGAIN country list does not spell out.
const COUNTRY_ALIASES: &[(&str, &[&str])] = &[
    (
        "US",
        &["united states", "usa", "u s", "american", "america"],
    ),
    (
        "GB",
        &[
            "uk",
            "united kingdom",
            "britain",
            "great britain",
            "british",
            "england",
            "scotland",
            "wales",
        ],
    ),
    ("SE", &["swedish"]),
    ("DE", &["german"]),
    ("FR", &["french"]),
    ("NL", &["dutch", "netherlands"]),
    ("IT", &["italian"]),
    ("ES", &["spanish"]),
    ("CN", &["chinese"]),
    ("JP", &["japanese"]),
    ("AU", &["australian"]),
    ("CA", &["canadian"]),
    ("FI", &["finnish"]),
    ("NO", &["norwegian"]),
    ("DK", &["danish"]),
    ("CH", &["swiss"]),
    ("AT", &["austrian"]),
    ("BR", &["brazilian"]),
    ("IN", &["indian"]),
    ("IE", &["irish"]),
    ("PL", &["polish"]),
    ("CZ", &["czech"]),
    ("HU", &["hungarian"]),
    ("GR", &["greek"]),
    ("PT", &["portuguese"]),
    ("BE", &["belgian"]),
    ("KR", &["korean", "south korea"]),
    ("NZ", &["kiwi"]),
    ("ZA", &["south african"]),
    ("IL", &["israeli"]),
    ("TR", &["turkish"]),
    ("MX", &["mexican"]),
    ("SI", &["slovenian"]),
    ("HR", &["croatian"]),
    ("EE", &["estonian"]),
    ("LV", &["latvian"]),
    ("LT", &["lithuanian"]),
    ("UG", &["ugandan"]),
    ("KE", &["kenyan"]),
    ("CL", &["chilean"]),
    ("CO", &["colombian"]),
];

const CATEGORY_ALIASES: &[(&str, &[&str])] = &[
    (
        "research-and-scholarship",
        &[
            "research and scholarship",
            "research & scholarship",
            "r&s",
            "r & s",
            "r and s",
            "rands",
            "research-and-scholarship",
        ],
    ),
    (
        "code-of-conduct-v2",
        &[
            "code of conduct",
            "coco",
            "code of conduct v2",
            "coco v2",
            "data protection code of conduct",
            "gdpr code of conduct",
        ],
    ),
    (
        "code-of-conduct-v1",
        &["code of conduct v1", "coco v1", "geant code of conduct"],
    ),
    ("anonymous", &["anonymous access", "anonymous"]),
    (
        "pseudonymous",
        &["pseudonymous access", "pseudonymous", "pseudonymously"],
    ),
    (
        "personalized",
        &[
            "personalized access",
            "personalised access",
            "personalized",
            "personalised",
        ],
    ),
    (
        "hide-from-discovery",
        &["hide from discovery", "hidden from discovery"],
    ),
    ("sirtfi", &["sirtfi"]),
    ("sirtfi2", &["sirtfi v2", "sirtfi2", "sirtfi 2"]),
];

const AVAILABILITY_ALIASES: &[(Availability, &[&str])] = &[
    (
        Availability::IncommonOnly,
        &[
            "incommon only",
            "only in incommon",
            "incommon-only",
            "not exported to edugain",
            "not in edugain",
            "not available in edugain",
            "only incommon",
            "not shared with edugain",
            "domestic only",
        ],
    ),
    (
        Availability::IncommonAndEdugain,
        &[
            "exported to edugain",
            "incommon and edugain",
            "shared with edugain",
            "published to edugain",
            "incommon services in edugain",
            "incommon services available internationally",
        ],
    ),
    (
        Availability::EdugainOnly,
        &[
            "not in incommon",
            "outside incommon",
            "not available in incommon",
            "not available to incommon",
            "missing from incommon",
            "edugain only",
            "edugain-only",
            "only in edugain",
            "beyond incommon",
            "not visible in incommon",
            "excluded from incommon",
        ],
    ),
    (
        Availability::EdugainImportedByIncommon,
        &[
            "international services in incommon",
            "imported into incommon",
            "imported by incommon",
            "from other federations in incommon",
            "foreign services in incommon",
        ],
    ),
];

/// Ways to ask for, or leave out, test and development instances. "test" alone is
/// a subject ("test prep", "proctored tests"), and so is "production" ("video
/// production"), so each needs a word that makes it about the instance.
const ENVIRONMENT_ALIASES: &[(&str, &[&str])] = &[
    (
        "production",
        &[
            "only production",
            "production only",
            "production services only",
            "in production",
            "production instance",
            "production instances",
            "production version",
            "production versions",
            "production environment",
            "production environments",
            "prod",
            "prod instances",
            "live services",
            "live instances",
            "live versions",
            "only live",
        ],
    ),
    (
        "test",
        &[
            "test instance",
            "test instances",
            "test environment",
            "test environments",
            "test version",
            "test versions",
            "test copy",
            "test copies",
            "test sp",
            "test sps",
            "test site",
            "test sites",
            "test server",
            "test servers",
            "test system",
            "test systems",
            "testing instance",
            "testing instances",
            "testing environment",
            "sandboxes",
            "sandbox instance",
            "sandbox instances",
            "sandbox environment",
            "qa instance",
            "qa instances",
            "qa environment",
            "uat",
            "uat instances",
        ],
    ),
    (
        "staging",
        &[
            "staging instance",
            "staging instances",
            "staging environment",
            "staging environments",
            "staging sites",
            "staging servers",
            "staging copies",
            "staging versions",
            "stage instances",
            "preprod",
            "pre-production",
            "pre production",
        ],
    ),
    (
        "development",
        &[
            "dev instance",
            "dev instances",
            "dev environment",
            "dev environments",
            "dev sites",
            "dev servers",
            "dev versions",
            "development instance",
            "development instances",
            "development environment",
            "development environments",
            "development servers",
            "development sites",
        ],
    ),
    (
        "test,staging",
        &[
            "test or staging instances",
            "test and staging instances",
            "staging or test instances",
            "test and staging environments",
        ],
    ),
    (
        "test,staging,development",
        &[
            "non-production",
            "nonproduction",
            "non production",
            "non-prod",
            "nonprod",
            "not production",
            "not in production",
        ],
    ),
];

/// "no test instances", "excluding staging", "without dev sites": what is left is
/// the live services and the ones nothing marks.
const ENVIRONMENT_NEGATIONS: &[&str] = &[
    "no",
    "not",
    "excluding",
    "exclude",
    "without",
    "hide",
    "skip",
];
const ENVIRONMENT_EXCLUDED: &[&str] = &[
    "test",
    "testing",
    "test instances",
    "test sites",
    "test environments",
    "test or staging",
    "test and staging",
    "test or dev",
    "test and dev",
    "staging",
    "dev",
    "dev instances",
    "development instances",
    "non-production",
    "nonproduction",
    "sandboxes",
];

/// Who a service is for: anyone's institution, or one institution's.
const SHARING_ALIASES: &[(&str, &[&str])] = &[
    (
        "multilateral",
        &[
            "multilateral",
            "anyone in edugain can use",
            "anyone in incommon can use",
            "any institution can use",
            "anyone can use",
            "open to any institution",
            "open to all institutions",
            "available to any institution",
            "usable by any institution",
            "for any institution",
        ],
    ),
    (
        "bilateral",
        &[
            "bilateral",
            "institution-specific",
            "institution specific",
            "single-institution",
            "single institution",
            "for one institution",
            "per-campus",
            "per campus",
            "campus-specific",
        ],
    ),
    (
        "multilateral,unknown",
        &[
            "not institution-specific",
            "not institution specific",
            "not specific to one institution",
            "not bilateral",
            "no bilateral",
            "excluding bilateral",
            "without bilateral",
            "no per-campus",
            "no per campus",
            "not per-campus",
        ],
    ),
];

/// Who runs a service. A sector needs a word that makes it about the organization:
/// "library" alone is a subject ("library management system"), while "libraries",
/// "library services", and "run by libraries" are who runs it.
const NOT_COMMERCIAL: &str =
    "academia,research,libraries,museums,government,healthcare,k12,nonprofit,nren";
const SECTOR_ALIASES: &[(&str, &[&str])] = &[
    (
        "academia",
        &[
            "universities",
            "colleges and universities",
            "academic institutions",
            "higher education institutions",
            "run by universities",
            "academia",
        ],
    ),
    (
        "research",
        &[
            "research institutes",
            "research institutions",
            "research labs",
            "research laboratories",
            "national labs",
            "national laboratories",
            "research centers",
            "research centres",
            "research infrastructures",
        ],
    ),
    (
        "commercial",
        &[
            "commercial",
            "commercial vendors",
            "commercial services",
            "commercial providers",
            "vendors",
            "vendor",
            "companies",
            "for-profit",
        ],
    ),
    (
        "nonprofit",
        &[
            "nonprofits",
            "non-profits",
            "nonprofit organizations",
            "charities",
            "foundations",
        ],
    ),
    (
        NOT_COMMERCIAL,
        &[
            "non-commercial",
            "noncommercial",
            "not commercial",
            "nonprofit",
            "non-profit",
        ],
    ),
    (
        "libraries",
        &[
            "libraries",
            "library services",
            "library sps",
            "run by libraries",
        ],
    ),
    (
        "museums",
        &[
            "museums",
            "run by museums",
            "museum services",
            "museum sps",
            "archives",
            "galleries",
            "glam",
            "cultural heritage institutions",
        ],
    ),
    (
        "government",
        &[
            "government",
            "government agencies",
            "government services",
            "public sector",
            "public bodies",
        ],
    ),
    (
        "healthcare",
        &[
            "hospitals",
            "health systems",
            "medical centers",
            "healthcare providers",
        ],
    ),
    ("k12", &["k-12", "k12", "schools", "school districts"]),
    (
        "publishers",
        &["publishers", "journal publishers", "publisher services"],
    ),
    (
        "nren",
        &[
            "nrens",
            "research and education networks",
            "federation operators",
            "network operators",
        ],
    ),
];

fn sector_label(value: &str) -> String {
    if value == NOT_COMMERCIAL {
        return "Anything but companies".into();
    }
    value
        .split(',')
        .map(|k| Sector::from_key(k).map(|s| s.label()).unwrap_or(k))
        .collect::<Vec<_>>()
        .join(" or ")
}

fn sharing_label(value: &str) -> String {
    value
        .split(',')
        .map(|k| Sharing::from_key(k).map(|s| s.label()).unwrap_or(k))
        .collect::<Vec<_>>()
        .join(" or ")
}

/// "test or staging": several environments in one filter.
fn environment_label(value: &str) -> String {
    value
        .split(',')
        .map(|k| Environment::from_key(k).map(|e| e.label()).unwrap_or(k))
        .collect::<Vec<_>>()
        .join(" or ")
}

const SOURCE_ALIASES: &[(&str, &[&str])] = &[
    (
        "incommon",
        &[
            "available in incommon",
            "available to incommon",
            "in incommon",
            "visible in incommon",
            "incommon users",
            "for incommon",
            "to incommon",
            "incommon metadata",
        ],
    ),
    (
        "edugain",
        &[
            "in edugain",
            "available in edugain",
            "edugain",
            "edugain services",
            "interfederation",
            "internationally available",
        ],
    ),
];

impl Vocab {
    pub fn new(services: &[Service], federations: &[Federation]) -> Self {
        let mut v = Vocab::default();

        for f in federations {
            v.federations
                .insert(f.code.clone(), (f.name.clone(), f.country_codes.clone()));
            for (i, cc) in f.country_codes.iter().enumerate() {
                if let Some(name) = f.countries.get(i) {
                    v.countries
                        .entry(cc.clone())
                        .or_insert_with(|| name.clone());
                }
            }
        }
        for (code, (name, _)) in v.federations.clone() {
            let mut spellings = vec![code.clone(), name.clone()];
            if code == "UK-FEDERATION" {
                spellings.extend(
                    ["uk federation", "ukamf", "uk access management federation"].map(String::from),
                );
            }
            if code == "INCOMMON" {
                spellings.extend(
                    [
                        "registered by incommon",
                        "incommon registered",
                        "incommon-registered",
                        "incommon",
                    ]
                    .map(String::from),
                );
            }
            for s in spellings {
                v.add(&s, Field::Federation, &code, &name, false);
                if let Some(p) = v.phrases.last_mut() {
                    if p.field == Field::Federation
                        && p.words.len() == 1
                        && p.words[0].chars().count() <= 4
                    {
                        p.shouty = true;
                    }
                }
            }
        }
        for (cc, name) in v.countries.clone() {
            v.add(&name, Field::Country, &cc, &name, false);
        }
        for (cc, names) in COUNTRY_ALIASES {
            if let Some(label) = v.countries.get(*cc).cloned() {
                for n in *names {
                    v.add(n, Field::Country, cc, &label, false);
                }
            }
        }
        for r in REGIONS.iter() {
            for n in [r.label.as_str(), r.label.trim_start_matches("the ")]
                .into_iter()
                .chain(r.aliases.iter().map(String::as_str))
            {
                v.add(n, Field::Region, &r.key, &r.label, false);
            }
        }
        for def in CATEGORIES {
            v.add(def.label, Field::Category, def.key, def.label, false);
        }
        for (key, names) in CATEGORY_ALIASES {
            if let Some(def) = CATEGORIES.iter().find(|d| d.key == *key) {
                for n in *names {
                    v.add(n, Field::Category, key, def.label, false);
                }
            }
        }
        for (a, names) in AVAILABILITY_ALIASES {
            for n in *names {
                v.add(n, Field::Availability, a.key(), a.label(), false);
            }
        }
        for (env, names) in ENVIRONMENT_ALIASES {
            for n in *names {
                v.add(n, Field::Environment, env, &environment_label(env), false);
            }
        }
        for (value, names) in SECTOR_ALIASES {
            for n in *names {
                v.add(n, Field::Sector, value, &sector_label(value), false);
            }
        }
        for (value, names) in SHARING_ALIASES {
            for n in *names {
                v.add(n, Field::Sharing, value, &sharing_label(value), false);
            }
        }
        for neg in ENVIRONMENT_NEGATIONS {
            for what in ENVIRONMENT_EXCLUDED {
                let value = "production,unknown";
                v.add(
                    &format!("{neg} {what}"),
                    Field::Environment,
                    value,
                    &environment_label(value),
                    false,
                );
            }
        }
        for (src, names) in SOURCE_ALIASES {
            let label = if *src == "incommon" {
                "Seen in InCommon's aggregate"
            } else {
                "Seen in eduGAIN's aggregate"
            };
            for n in *names {
                v.add(n, Field::Source, src, label, false);
            }
        }

        // Requested attributes: one Name per friendly name, the most requested one.
        let mut counts: HashMap<(&str, Option<&str>), usize> = HashMap::new();
        for s in services {
            for a in &s.requested_attributes {
                *counts
                    .entry((a.name.as_str(), a.friendly_name.as_deref()))
                    .or_default() += 1;
            }
        }
        let mut ranked: Vec<_> = counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let mut by_friendly: HashMap<String, String> = HashMap::new();
        for ((name, friendly), _) in &ranked {
            if !v.attributes.iter().any(|(n, _)| n == name) {
                v.attributes
                    .push((name.to_string(), friendly.map(str::to_owned)));
            }
            if let Some(f) = friendly {
                by_friendly
                    .entry(f.to_lowercase())
                    .or_insert_with(|| name.to_string());
            }
        }
        for (friendly, names, unambiguous) in ATTRIBUTE_ALIASES {
            let key = friendly.to_lowercase();
            // pairwise-id and subject-id usually travel without a friendly name.
            let name = by_friendly.get(&key).cloned().or_else(|| {
                v.attributes
                    .iter()
                    .find(|(n, _)| n.ends_with(&format!(":{friendly}")))
                    .map(|(n, _)| n.clone())
            });
            if let Some(name) = name {
                for n in *names {
                    v.add(n, Field::Attribute, &name, friendly, !unambiguous);
                }
            }
        }
        for (friendly, name) in &by_friendly {
            if friendly.len() > 3 {
                let label = v
                    .attributes
                    .iter()
                    .find(|(n, _)| n == name)
                    .and_then(|(_, f)| f.clone())
                    .unwrap_or_default();
                v.add(friendly, Field::Attribute, name, &label, true);
            }
        }

        for s in services {
            if let Some(t) = s.curated.as_ref().and_then(|c| c.provider_type.as_ref()) {
                v.provider_types
                    .entry(t.to_lowercase())
                    .or_insert_with(|| t.clone());
            }
        }
        for (lc, t) in v.provider_types.clone() {
            v.add(&lc, Field::ProviderType, &lc, &t, false);
            if lc == "commercial" {
                v.add("vendor", Field::ProviderType, &lc, &t, false);
                v.add("vendors", Field::ProviderType, &lc, &t, false);
            }
        }

        // Longest phrases first so "code of conduct v1" beats "code of conduct".
        v.phrases.sort_by(|a, b| {
            b.words
                .len()
                .cmp(&a.words.len())
                .then(a.field.cmp(&b.field))
        });
        v
    }

    fn add(&mut self, text: &str, field: Field, value: &str, label: &str, needs_cue: bool) {
        let words = words(text);
        if words.is_empty()
            || self
                .phrases
                .iter()
                .any(|p| p.words == words && p.field == field)
        {
            return;
        }
        self.phrases.push(Phrase {
            words,
            field,
            value: value.to_owned(),
            label: label.to_owned(),
            needs_cue,
            shouty: false,
        });
    }

    /// Whether any filter phrase begins at word `i`.
    fn phrase_starts(&self, toks: &[String], i: usize) -> bool {
        self.phrases.iter().any(|p| {
            let n = p.words.len();
            i + n <= toks.len() && toks[i..i + n] == p.words[..]
        })
    }

    /// Read a query with the built-in rules.
    pub fn interpret(&self, query: &str) -> Interpretation {
        let cased = cased_words(query);
        let toks: Vec<String> = cased.iter().map(|w| w.to_lowercase()).collect();
        let mut used = vec![false; toks.len()];
        let mut matched: Vec<MatchedTerm> = Vec::new();
        let mut notes = Vec::new();

        let mut i = 0;
        while i < toks.len() {
            let hit = self.phrases.iter().find(|p| {
                let n = p.words.len();
                i + n <= toks.len()
                    && toks[i..i + n] == p.words[..]
                    && !used[i..i + n].iter().any(|u| *u)
                    && (!p.needs_cue || cue_before(&toks, i))
                    && (!p.shouty || shouted(&cased[i]) || beside(&toks, i, "federation"))
            });
            // "European Molecular Biology Laboratory", "Europe PMC", "American Chemical
            // Society": a capitalized place followed by a capitalized word that is no
            // filter of its own is part of a name.
            let place = |p: &&Phrase| matches!(p.field, Field::Country | Field::Region);
            if let Some(p) = hit.filter(place) {
                let next = i + p.words.len();
                let capital = |w: &str| w.chars().next().is_some_and(char::is_uppercase);
                if capital(&cased[i])
                    && cased.get(next).is_some_and(|w| capital(w))
                    && !self.phrase_starts(&toks, next)
                {
                    notes.push(format!(
                        "\"{}\" looks like part of a name (\"{} {} …\"), so it was searched as a word rather than as a place.",
                        cased[i..next].join(" "),
                        cased[i..next].join(" "),
                        cased[next]
                    ));
                    i = next;
                    continue;
                }
            }
            match hit {
                Some(p) => {
                    if p.field == Field::Country && toks[i..i + p.words.len()] == ["america"] {
                        notes.push("\"America\" was read as the United States; say \"the Americas\" for North and South America.".into());
                    }
                    if let Some(note) = (p.field == Field::Region)
                        .then(|| regions::get(&p.value).and_then(|r| r.reading_note.as_deref()))
                        .flatten()
                    {
                        notes.push(note.into());
                    }
                    for u in &mut used[i..i + p.words.len()] {
                        *u = true;
                    }
                    matched.push(MatchedTerm {
                        text: toks[i..i + p.words.len()].join(" "),
                        field: p.field.key().to_owned(),
                        value: p.value.clone(),
                        label: p.label.clone(),
                    });
                    i += p.words.len();
                }
                None => i += 1,
            }
        }

        let drop_provider_words = self.provider_types.is_empty();
        if drop_provider_words {
            for (t, _) in toks
                .iter()
                .zip(&used)
                .filter(|(t, u)| !**u && PROVIDER_TYPE_WORDS.contains(&t.as_str()))
            {
                notes.push(format!(
                    "\"{t}\" was ignored: federation metadata does not say whether a provider is commercial, and no curated provider types are loaded."
                ));
            }
        }
        let is_content = |t: &str| {
            let filler = STOPWORDS.contains(&t) || CUES.contains(&t);
            let provider_word = drop_provider_words && PROVIDER_TYPE_WORDS.contains(&t);
            !(filler || provider_word)
        };
        // Each unbroken run of words that no filter claimed becomes one phrase:
        // its content words, plus linking words that sit between two of them.
        let mut phrases: Vec<String> = Vec::new();
        for run in toks
            .iter()
            .zip(&used)
            .collect::<Vec<_>>()
            .split(|(_, u)| **u)
        {
            let run: Vec<&str> = run.iter().map(|(t, _)| t.as_str()).collect();
            let (Some(first), Some(last)) = (
                run.iter().position(|t| is_content(t)),
                run.iter().rposition(|t| is_content(t)),
            ) else {
                continue;
            };
            let kept: Vec<&str> = run[first..=last]
                .iter()
                .copied()
                .filter(|t| is_content(t) || LINKS.contains(t))
                .collect();
            phrases.push(kept.join(" "));
        }

        let params = self.collect(&matched, &phrases.join(" "), &mut notes);
        Interpretation {
            query: query.trim().to_owned(),
            engine: "rules".into(),
            params,
            matched,
            notes,
            explanation: None,
        }
    }

    /// Fold matches into one value per field (categories, federations, places, and
    /// environments combine; the rest take the first match and note what was dropped).
    fn collect(
        &self,
        matched: &[MatchedTerm],
        keywords: &str,
        notes: &mut Vec<String>,
    ) -> InterpretedParams {
        let mut p = InterpretedParams {
            q: (!keywords.is_empty()).then(|| keywords.to_owned()),
            ..Default::default()
        };
        fn add<'a>(list: &mut Vec<&'a str>, v: &'a str) {
            if !list.contains(&v) {
                list.push(v);
            }
        }
        let mut cats: Vec<&str> = Vec::new();
        let mut feds: Vec<&str> = Vec::new();
        let mut envs: Vec<&str> = Vec::new();
        let mut sharing: Vec<&str> = Vec::new();
        let mut sectors: Vec<&str> = Vec::new();
        let mut countries: Vec<&str> = Vec::new();
        let mut regions: Vec<&str> = Vec::new();
        for m in matched {
            let slot = match m.field.as_str() {
                "country" => {
                    add(&mut countries, &m.value);
                    continue;
                }
                "region" => {
                    add(&mut regions, &m.value);
                    continue;
                }
                "environment" => {
                    for v in m.value.split(',') {
                        add(&mut envs, v);
                    }
                    continue;
                }
                "sharing" => {
                    for v in m.value.split(',') {
                        add(&mut sharing, v);
                    }
                    continue;
                }
                "sector" => {
                    for v in m.value.split(',') {
                        add(&mut sectors, v);
                    }
                    continue;
                }
                "category" => {
                    add(&mut cats, &m.value);
                    continue;
                }
                "federation" => {
                    add(&mut feds, &m.value);
                    continue;
                }
                "availability" => &mut p.availability,
                "attribute" => &mut p.attribute,
                "source" => &mut p.source,
                "provider_type" => &mut p.provider_type,
                _ => continue,
            };
            match slot {
                None => *slot = Some(m.value.clone()),
                Some(existing) if *existing != m.value => notes.push(format!(
                    "Only one {} can be filtered at a time, so \"{}\" was not applied.",
                    m.field.replace('_', " "),
                    m.text
                )),
                Some(_) => {}
            }
        }
        if !cats.is_empty() {
            p.category = Some(cats.join(","));
        }
        if !feds.is_empty() {
            p.federation = Some(feds.join(","));
        }
        if !envs.is_empty() {
            p.environment = Some(envs.join(","));
        }
        if !sharing.is_empty() {
            p.sharing = Some(sharing.join(","));
        }
        if !sectors.is_empty() {
            p.sector = Some(sectors.join(","));
        }
        if !countries.is_empty() {
            p.country = Some(countries.join(","));
        }
        if !regions.is_empty() {
            p.region = Some(regions.join(","));
        }
        // An availability already says which aggregates; a source on top is redundant
        // and can contradict it ("not in InCommon" contains "in InCommon").
        if p.availability.is_some() {
            p.source = None;
        }
        p
    }

    /// Hold model-produced filters to the catalog's vocabulary.
    pub fn validate(&self, p: InterpretedParams, notes: &mut Vec<String>) -> InterpretedParams {
        let mut keep = |field: &str, v: Option<String>, ok: &dyn Fn(&str) -> bool| match v {
            Some(v) if ok(&v) => Some(v),
            Some(v) => {
                notes.push(format!("Dropped an unknown {field} \"{v}\"."));
                None
            }
            None => None,
        };
        let category = p.category.map(|c| {
            c.split(',')
                .map(str::trim)
                .filter(|k| CATEGORIES.iter().any(|d| d.key == *k))
                .collect::<Vec<_>>()
                .join(",")
        });
        let federations: Vec<String> = split_list(p.federation.as_deref())
            .into_iter()
            .filter_map(|v| keep("federation", Some(v), &|v| self.federations.contains_key(v)))
            .collect();
        let environments: Vec<String> =
            split_list(p.environment.map(|e| e.to_lowercase()).as_deref())
                .into_iter()
                .filter_map(|v| {
                    keep("environment", Some(v), &|v| {
                        Environment::from_key(v).is_some()
                    })
                })
                .collect();
        let countries: Vec<String> = split_list(p.country.map(|c| c.to_uppercase()).as_deref())
            .into_iter()
            .filter_map(|v| keep("country", Some(v), &|v| self.countries.contains_key(v)))
            .collect();
        let regions: Vec<String> = split_list(p.region.map(|r| r.to_lowercase()).as_deref())
            .into_iter()
            .filter_map(|v| keep("region", Some(v), &|v| regions::get(v).is_some()))
            .collect();
        let sharing: Vec<String> = split_list(p.sharing.map(|v| v.to_lowercase()).as_deref())
            .into_iter()
            .filter_map(|v| keep("sharing", Some(v), &|v| Sharing::from_key(v).is_some()))
            .collect();
        let sectors: Vec<String> = split_list(p.sector.map(|v| v.to_lowercase()).as_deref())
            .into_iter()
            .filter_map(|v| {
                keep("sector", Some(v), &|v| {
                    v == "unknown" || Sector::from_key(v).is_some()
                })
            })
            .collect();
        InterpretedParams {
            q: p.q.map(|q| q.trim().to_owned()).filter(|q| !q.is_empty()),
            sector: (!sectors.is_empty()).then(|| sectors.join(",")),
            sharing: (!sharing.is_empty()).then(|| sharing.join(",")),
            country: (!countries.is_empty()).then(|| countries.join(",")),
            region: (!regions.is_empty()).then(|| regions.join(",")),
            environment: (!environments.is_empty()).then(|| environments.join(",")),
            federation: (!federations.is_empty()).then(|| federations.join(",")),
            category: category.filter(|c| !c.is_empty()),
            availability: keep("availability", p.availability, &|v| {
                Availability::from_key(v).is_some()
            }),
            attribute: keep("attribute", p.attribute, &|v| {
                self.attributes.iter().any(|(n, _)| n == v)
            }),
            source: keep("source", p.source, &|v| v == "incommon" || v == "edugain"),
            provider_type: keep(
                "provider type",
                p.provider_type.map(|t| t.to_lowercase()),
                &|v| self.provider_types.contains_key(v),
            ),
        }
    }

    /// A label for a filter value, for describing a model's reading.
    pub fn label(&self, field: &str, value: &str) -> String {
        match field {
            "federation" => self.federations.get(value).map(|(n, _)| n.clone()),
            "country" => self.countries.get(value).cloned(),
            "region" => regions::get(value).map(|r| r.label.to_owned()),
            "category" => CATEGORIES
                .iter()
                .find(|d| d.key == value)
                .map(|d| d.label.to_owned()),
            "availability" => Availability::from_key(value).map(|a| a.label().to_owned()),
            "environment" => Some(environment_label(value)),
            "sharing" => Some(sharing_label(value)),
            "sector" => Some(sector_label(value)),
            "attribute" => self
                .attributes
                .iter()
                .find(|(n, _)| n == value)
                .and_then(|(_, f)| f.clone()),
            "source" => Some(
                if value == "incommon" {
                    "Seen in InCommon's aggregate"
                } else {
                    "Seen in eduGAIN's aggregate"
                }
                .to_owned(),
            ),
            "provider_type" => self.provider_types.get(value).cloned(),
            _ => None,
        }
        .unwrap_or_else(|| value.to_owned())
    }
}

fn cue_before(toks: &[String], i: usize) -> bool {
    toks[i.saturating_sub(4)..i]
        .iter()
        .any(|t| CUES.contains(&t.as_str()))
}

/// Typed with at least two capitals: "LEAF", "CAFe", "SIR".
fn shouted(word: &str) -> bool {
    word.chars().filter(|c| c.is_uppercase()).count() >= 2
}

fn beside(toks: &[String], i: usize, word: &str) -> bool {
    (i > 0 && toks[i - 1] == word) || toks.get(i + 1).is_some_and(|t| t == word)
}

/// Lowercase words, keeping `&` (R&S) and inner hyphens, dropping other punctuation.
fn words(s: &str) -> Vec<String> {
    cased_words(s)
        .into_iter()
        .map(|w| w.to_lowercase())
        .collect()
}

/// [`words`] before lowercasing, so capitalization can be checked.
fn cased_words(s: &str) -> Vec<String> {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '&' || c == '-' {
                c
            } else {
                ' '
            }
        })
        .collect();
    cleaned
        .split_whitespace()
        .map(|w| w.trim_matches('-').to_owned())
        .filter(|w| !w.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RequestedAttribute;

    fn vocab() -> Vocab {
        let s = Service {
            requested_attributes: vec![
                RequestedAttribute {
                    name: "urn:oid:0.9.2342.19200300.100.1.3".into(),
                    friendly_name: Some("mail".into()),
                    ..Default::default()
                },
                RequestedAttribute {
                    name: "urn:oid:1.3.6.1.4.1.5923.1.1.1.6".into(),
                    friendly_name: Some("eduPersonPrincipalName".into()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let feds = vec![
            Federation {
                code: "SWAMID".into(),
                name: "SWAMID".into(),
                country_codes: vec!["SE".into()],
                countries: vec!["Sweden".into()],
                ..Default::default()
            },
            Federation {
                code: "INCOMMON".into(),
                name: "InCommon".into(),
                country_codes: vec!["US".into()],
                countries: vec!["United States".into()],
                ..Default::default()
            },
            Federation {
                code: "UK-FEDERATION".into(),
                name: "UK federation".into(),
                country_codes: vec!["GB".into()],
                countries: vec!["United Kingdom".into()],
                ..Default::default()
            },
        ];
        Vocab::new(&[s], &feds)
    }

    #[test]
    fn reads_filters_and_keeps_subject_words() {
        let i = vocab().interpret("R&S wiki services in Sweden that request email");
        assert_eq!(
            i.params.category.as_deref(),
            Some("research-and-scholarship")
        );
        assert_eq!(i.params.country.as_deref(), Some("SE"));
        assert_eq!(
            i.params.attribute.as_deref(),
            Some("urn:oid:0.9.2342.19200300.100.1.3")
        );
        assert_eq!(i.params.q.as_deref(), Some("wiki"));

        // "library services" is who runs them.
        let i = vocab().interpret("R&S library services in Sweden that request email");
        assert_eq!(i.params.sector.as_deref(), Some("libraries"));
        assert_eq!(i.params.country.as_deref(), Some("SE"));
        assert_eq!(i.params.q, None);
    }

    #[test]
    fn email_without_a_cue_is_a_subject() {
        let i = vocab().interpret("email marketing tools");
        assert_eq!(i.params.attribute, None);
        assert_eq!(i.params.q.as_deref(), Some("email marketing tools"));
    }

    #[test]
    fn availability_beats_federation_and_source() {
        let i = vocab().interpret("journals not in InCommon");
        assert_eq!(i.params.availability.as_deref(), Some("edugain-only"));
        assert_eq!(i.params.federation, None);
        assert_eq!(i.params.source, None);
        assert_eq!(i.params.q.as_deref(), Some("journals"));

        let i = vocab().interpret("UK federation code of conduct services exported to eduGAIN");
        assert_eq!(i.params.federation.as_deref(), Some("UK-FEDERATION"));
        assert_eq!(i.params.category.as_deref(), Some("code-of-conduct-v2"));
        assert_eq!(
            i.params.availability.as_deref(),
            Some("incommon-and-edugain")
        );
        assert_eq!(i.params.q, None);
    }

    #[test]
    fn several_federations_combine() {
        let i = vocab().interpret("R&S services registered by SWAMID or InCommon");
        assert_eq!(i.params.federation.as_deref(), Some("SWAMID,INCOMMON"));
        assert_eq!(
            i.params.category.as_deref(),
            Some("research-and-scholarship")
        );
        assert_eq!(i.params.q, None);
        assert!(i.notes.is_empty(), "{:?}", i.notes);
        assert_eq!(
            i.matched.iter().filter(|m| m.field == "federation").count(),
            2
        );

        // Naming one twice is still one.
        let i = vocab().interpret("SWAMID wikis, swamid only");
        assert_eq!(i.params.federation.as_deref(), Some("SWAMID"));
    }

    #[test]
    fn short_federation_codes_need_capitals_or_context() {
        let feds = vec![
            Federation {
                code: "LEAF".into(),
                name: "LEAF".into(),
                country_codes: vec!["LU".into()],
                countries: vec!["Luxembourg".into()],
                ..Default::default()
            },
            Federation {
                code: "SIR".into(),
                name: "SIR".into(),
                country_codes: vec!["ES".into()],
                countries: vec!["Spain".into()],
                ..Default::default()
            },
        ];
        let v = Vocab::new(&[], &feds);
        assert_eq!(
            v.interpret("leaf identification database")
                .params
                .federation,
            None
        );
        assert_eq!(
            v.interpret("sir isaac newton papers").params.federation,
            None
        );
        assert_eq!(
            v.interpret("services in LEAF").params.federation.as_deref(),
            Some("LEAF")
        );
        assert_eq!(
            v.interpret("the sir federation")
                .params
                .federation
                .as_deref(),
            Some("SIR")
        );
    }

    #[test]
    fn keeps_phrases_whole_and_drops_audience_words() {
        let v = vocab();
        assert_eq!(
            v.interpret("web of science").params.q.as_deref(),
            Some("web of science")
        );
        assert_eq!(
            v.interpret("where can I publish research data")
                .params
                .q
                .as_deref(),
            Some("publish research data")
        );
        assert_eq!(
            v.interpret("something like overleaf for swedish researchers")
                .params
                .q
                .as_deref(),
            Some("overleaf")
        );
    }

    #[test]
    fn plain_keywords_are_left_alone() {
        let i = vocab().interpret("zoom");
        assert!(!i.has_filters());
        assert_eq!(i.params.q.as_deref(), Some("zoom"));
    }

    #[test]
    fn commercial_reads_as_a_sector() {
        let i = vocab().interpret("commercial british services that need eppn");
        assert_eq!(i.params.country.as_deref(), Some("GB"));
        assert_eq!(
            i.params.attribute.as_deref(),
            Some("urn:oid:1.3.6.1.4.1.5923.1.1.1.6")
        );
        assert_eq!(i.params.sector.as_deref(), Some("commercial"));
        assert!(i.notes.is_empty(), "{:?}", i.notes);
        assert_eq!(i.params.q, None);
    }

    #[test]
    fn sectors_by_name() {
        let v = vocab();
        let i = v.interpret("services from libraries");
        assert_eq!(i.params.sector.as_deref(), Some("libraries"));
        assert_eq!(i.params.q, None);
        let i = v.interpret("SPs run by museums");
        assert_eq!(i.params.sector.as_deref(), Some("museums"));
        let i = v.interpret("commercial vendors in Europe");
        assert_eq!(i.params.sector.as_deref(), Some("commercial"));
        assert_eq!(i.params.region.as_deref(), Some("europe"));
        let i = v.interpret("non-commercial research tools");
        assert_eq!(i.params.sector.as_deref(), Some(NOT_COMMERCIAL));
        assert_eq!(i.params.q.as_deref(), Some("research tools"));
        assert_eq!(i.matched[0].label, "Anything but companies");
        let i = v.interpret("government agencies or hospitals");
        assert_eq!(i.params.sector.as_deref(), Some("government,healthcare"));
        for (q, sector) in [
            ("library services", "libraries"),
            ("German library SPs", "libraries"),
            ("museum SPs", "museums"),
            ("museum services in Europe", "museums"),
        ] {
            assert_eq!(v.interpret(q).params.sector.as_deref(), Some(sector), "{q}");
        }
        // A subject is not who runs it.
        for q in [
            "library management system",
            "museum ticketing",
            "research data",
        ] {
            assert_eq!(v.interpret(q).params.sector, None, "{q}");
        }
        let mut notes = Vec::new();
        let p = v.validate(
            InterpretedParams {
                sector: Some("Academia,banks,unknown".into()),
                ..Default::default()
            },
            &mut notes,
        );
        assert_eq!(p.sector.as_deref(), Some("academia,unknown"));
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn validate_drops_values_outside_the_catalog() {
        let v = vocab();
        let mut notes = Vec::new();
        let p = v.validate(
            InterpretedParams {
                country: Some("se".into()),
                federation: Some("NOPE".into()),
                category: Some("sirtfi,made-up".into()),
                ..Default::default()
            },
            &mut notes,
        );
        assert_eq!(p.country.as_deref(), Some("SE"));
        assert_eq!(p.federation, None);
        assert_eq!(p.category.as_deref(), Some("sirtfi"));
        assert_eq!(notes.len(), 1);

        let mut notes = Vec::new();
        let p = v.validate(
            InterpretedParams {
                federation: Some("SWAMID, NOPE,INCOMMON,SWAMID".into()),
                ..Default::default()
            },
            &mut notes,
        );
        assert_eq!(p.federation.as_deref(), Some("SWAMID,INCOMMON"));
        assert_eq!(notes, ["Dropped an unknown federation \"NOPE\"."]);
    }

    #[test]
    fn environments_are_read_with_their_instance_words() {
        let v = vocab();
        let i = v.interpret("test instances of Canvas");
        assert_eq!(i.params.environment.as_deref(), Some("test"));
        assert_eq!(i.params.q.as_deref(), Some("canvas"));
        assert_eq!(i.matched[0].label, "Test");

        let i = v.interpret("only production services");
        assert_eq!(i.params.environment.as_deref(), Some("production"));
        assert_eq!(i.params.q, None);

        let i = v.interpret("test or staging instances of zoom");
        assert_eq!(i.params.environment.as_deref(), Some("test,staging"));
        assert_eq!(i.matched[0].label, "Test or Staging");

        let i = v.interpret("non-production wikis");
        assert_eq!(
            i.params.environment.as_deref(),
            Some("test,staging,development")
        );
        assert_eq!(i.params.q.as_deref(), Some("wikis"));

        let i = v.interpret("R&S services in Sweden, no test instances");
        assert_eq!(i.params.environment.as_deref(), Some("production,unknown"));
        assert_eq!(i.params.country.as_deref(), Some("SE"));
        assert_eq!(
            i.params.category.as_deref(),
            Some("research-and-scholarship")
        );

        // Two readings combine, like federations.
        let i = v.interpret("dev instances and staging environments of the library proxy");
        assert_eq!(i.params.environment.as_deref(), Some("development,staging"));
        assert_eq!(i.params.q.as_deref(), Some("library proxy"));
    }

    #[test]
    fn test_and_production_as_subjects_are_left_alone() {
        let v = vocab();
        for q in [
            "test prep",
            "proctored test delivery",
            "video production services",
            "cancer staging calculator",
            "sandbox games",
            "music production",
            "testing center",
        ] {
            let i = v.interpret(q);
            assert_eq!(i.params.environment, None, "{q}");
        }
    }

    #[test]
    fn validate_keeps_known_environments() {
        let mut notes = Vec::new();
        let p = vocab().validate(
            InterpretedParams {
                environment: Some("Test, prod,staging,test".into()),
                ..Default::default()
            },
            &mut notes,
        );
        assert_eq!(p.environment.as_deref(), Some("test,staging"));
        assert_eq!(notes, ["Dropped an unknown environment \"prod\"."]);
        assert_eq!(
            vocab().label("environment", "production,unknown"),
            "Production or Unknown"
        );
    }

    #[test]
    fn regions_and_several_countries() {
        let v = vocab();
        let i = v.interpret("Research & Scholarship services in Europe");
        assert_eq!(i.params.region.as_deref(), Some("europe"));
        assert_eq!(
            i.params.category.as_deref(),
            Some("research-and-scholarship")
        );
        assert_eq!(i.params.q, None);

        for (q, region) in [
            ("SPs registered in the Americas", "americas"),
            ("anything from BRICS countries", "brics"),
            ("wikis in the EU", "eu"),
            ("european library services", "europe"),
            ("LatAm journals", "latin-america"),
            ("Asia-Pacific research data", "asia-pacific"),
            ("services in Oceania", "oceania"),
            ("the european union", "eu"),
        ] {
            assert_eq!(v.interpret(q).params.region.as_deref(), Some(region), "{q}");
        }

        let i = v.interpret("services in Sweden or the UK");
        assert_eq!(i.params.country.as_deref(), Some("SE,GB"));
        assert!(i.notes.is_empty(), "{:?}", i.notes);
        let i = v.interpret("services in Sweden or anywhere in the EEA");
        assert_eq!(i.params.country.as_deref(), Some("SE"));
        assert_eq!(i.params.region.as_deref(), Some("eea"));
    }

    #[test]
    fn ambiguous_place_names_say_how_they_were_read() {
        let v = vocab();
        let i = v.interpret("journals in America");
        assert_eq!(i.params.country.as_deref(), Some("US"));
        assert_eq!(i.params.region, None);
        assert!(
            i.notes.iter().any(|n| n.contains("the Americas")),
            "{:?}",
            i.notes
        );

        let i = v.interpret("North American wikis");
        assert_eq!(i.params.region.as_deref(), Some("northern-america"));
        assert!(
            i.notes.iter().any(|n| n.contains("Mexico")),
            "{:?}",
            i.notes
        );

        let i = v.interpret("APAC services");
        assert!(i.notes.iter().any(|n| n.contains("no standard definition")));
    }

    #[test]
    fn places_inside_names_are_words() {
        let v = vocab();
        for (q, words) in [
            (
                "European Molecular Biology Laboratory",
                "european molecular biology laboratory",
            ),
            ("Europe PMC", "europe pmc"),
            (
                "American Chemical Society journals",
                "american chemical society journals",
            ),
        ] {
            let i = v.interpret(q);
            assert_eq!(i.params.region, None, "{q}");
            assert_eq!(i.params.country, None, "{q}");
            assert_eq!(i.params.q.as_deref(), Some(words), "{q}");
            assert!(i.notes.iter().any(|n| n.contains("part of a name")), "{q}");
        }
        // A capital that starts another filter is not a name.
        let i = v.interpret("European R&S services");
        assert_eq!(i.params.region.as_deref(), Some("europe"));
        // Lowercase can't be told apart, and reads as a place.
        let i = v.interpret("european molecular biology");
        assert_eq!(i.params.region.as_deref(), Some("europe"));
    }

    #[test]
    fn validate_keeps_known_places() {
        let mut notes = Vec::new();
        let p = vocab().validate(
            InterpretedParams {
                country: Some("se,XX,GB".into()),
                region: Some("Europe,atlantis,eu".into()),
                ..Default::default()
            },
            &mut notes,
        );
        assert_eq!(p.country.as_deref(), Some("SE,GB"));
        assert_eq!(p.region.as_deref(), Some("europe,eu"));
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert_eq!(vocab().label("region", "brics"), "BRICS");
    }

    #[test]
    fn sharing_phrasings() {
        let v = vocab();
        let i = v.interpret("services anyone in eduGAIN can use for survey tools");
        assert_eq!(i.params.sharing.as_deref(), Some("multilateral"));
        assert_eq!(i.params.source, None, "the phrase claims \"in eduGAIN\"");
        assert_eq!(i.params.q.as_deref(), Some("survey tools"));

        let i = v.interpret("video conferencing, not institution-specific");
        assert_eq!(i.params.sharing.as_deref(), Some("multilateral,unknown"));
        assert_eq!(i.params.q.as_deref(), Some("video conferencing"));
        assert_eq!(i.matched[0].label, "Open to any institution or Unknown");

        let i = v.interpret("bilateral zoom registrations");
        assert_eq!(i.params.sharing.as_deref(), Some("bilateral"));

        let mut notes = Vec::new();
        let p = v.validate(
            InterpretedParams {
                sharing: Some("Multilateral,open".into()),
                ..Default::default()
            },
            &mut notes,
        );
        assert_eq!(p.sharing.as_deref(), Some("multilateral"));
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn groupings_by_name() {
        let v = vocab();
        for (q, region) in [
            ("R&S services in Schengen countries", "schengen"),
            ("services from NATO members", "nato"),
            ("OECD library services", "oecd"),
            ("wikis in the eurozone", "eurozone"),
            ("Nordic countries' research data", "nordic-council"),
            ("services in G20 countries", "g20"),
            ("ASEAN universities' services", "asean"),
            ("services registered in GÉANT countries", "geant"),
            ("UbuntuNet countries' services", "ubuntunet"),
            ("anything from Mercosur", "mercosur"),
            ("services from Commonwealth countries", "commonwealth"),
        ] {
            assert_eq!(v.interpret(q).params.region.as_deref(), Some(region), "{q}");
        }
        // Several groupings combine like countries.
        let i = v.interpret("services in NATO members or the GCC");
        assert_eq!(i.params.region.as_deref(), Some("nato,gcc"));
    }

    #[test]
    fn network_and_grouping_names_inside_service_names_stay_words() {
        let v = vocab();
        for q in [
            "RedCLARA communities",
            "virginia commonwealth university",
            "geant edusign",
            "apan conference",
        ] {
            let i = v.interpret(q);
            assert_eq!(i.params.region, None, "{q}");
        }
    }

    #[test]
    fn african_blocs_by_name() {
        let v = vocab();
        for (q, region) in [
            ("services in ECOWAS countries", "ecowas"),
            ("SADC members' library services", "sadc"),
            ("services from the East African Community", "eac"),
            ("anything in the CFA franc zone", "cfa-franc-zone"),
            ("Arab League countries", "arab-league"),
            ("COMESA services", "comesa"),
        ] {
            assert_eq!(v.interpret(q).params.region.as_deref(), Some(region), "{q}");
        }
        // Aligarh Muslim University's "AMU" is not the Arab Maghreb Union.
        assert_eq!(v.interpret("amu library").params.region, None);
    }
}
