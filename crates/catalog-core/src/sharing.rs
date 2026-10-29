// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Whether a service is open to users from any institution (multilateral) or
//! registered for one institution's users (bilateral).
//!
//! Much of the catalog is bilateral: hundreds of per-campus Zoom, Canvas, and
//! Qualtrics registrations, and campuses' own internal services. They are noise to
//! someone looking for a service they could use. A multilateral service accepts
//! users from any identity provider in principle; whether a particular person gets
//! in still depends on their identity provider releasing the attributes it needs,
//! and on any subscription it requires. So "multilateral" never promises access.
//!
//! The metadata gives facts ([`Signals`]), not answers. Two rules turn them into a
//! label when they are near certain; Claude reads the rest during ingest
//! ([`crate::model::SharingLabel`]), and the result records which decided:
//!
//! 1. **Tenant cluster** (`metadata`): one of five or more registrations that differ
//!    only by the first label of the host (`<campus>.zoom.us`), mostly by one
//!    registrant, not published to eduGAIN, and carrying no category that leans
//!    multilateral. Outranks Claude.
//! 2. **Claude's reading** (`generated`), given the same facts.
//! 3. **Open categories** (`metadata`): when Claude has not read it, an entry
//!    published to eduGAIN with Research & Scholarship or one of the REFEDS access
//!    categories, outside any tenant cluster, is multilateral.
//! 4. Otherwise `unknown`.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::categories;
use crate::model::{pick, Service, Source};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Sharing {
    Multilateral,
    Bilateral,
    Unknown,
}

impl Sharing {
    pub const ALL: [Sharing; 3] = [Sharing::Multilateral, Sharing::Bilateral, Sharing::Unknown];

    pub fn key(self) -> &'static str {
        match self {
            Sharing::Multilateral => "multilateral",
            Sharing::Bilateral => "bilateral",
            Sharing::Unknown => "unknown",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Sharing::Multilateral => "Open to any institution",
            Sharing::Bilateral => "For one institution",
            Sharing::Unknown => "Unknown",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == k)
    }
}

/// Fewest registrations that make a tenant cluster.
pub const CLUSTER_MIN: usize = 5;
/// Share of a cluster's registrations that must come from one registrant.
const CLUSTER_SAME_REGISTRANT: f64 = 0.8;

/// Registrations that differ only by tenant, such as `https://*.zoom.us`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Cluster {
    /// The entityID with the host's first label as `*`.
    pub pattern: String,
    pub size: usize,
    /// The organization most of them are registered under.
    pub registrant: String,
}

/// The categories that lean multilateral: they are for services that take users
/// from many identity providers.
const OPEN_CATEGORIES: &[&str] = &[
    "research-and-scholarship",
    "anonymous",
    "pseudonymous",
    "personalized",
    "code-of-conduct-v2",
    "code-of-conduct-v1",
];
/// Of those, the ones strong enough to decide without Claude (rule 3). The Code of
/// Conduct is often declared by a campus's own services too.
const DECIDING_CATEGORIES: &[&str] = &[
    "research-and-scholarship",
    "anonymous",
    "pseudonymous",
    "personalized",
];

/// The facts a label may rest on, by the key Claude cites them with.
pub const FACTS: &[(&str, &str)] = &[
    (
        "open_categories",
        "Entity categories that lean multilateral",
    ),
    ("in_edugain", "Published to eduGAIN"),
    (
        "discovery",
        "A discovery-response or request-initiator endpoint",
    ),
    (
        "institution_named",
        "One institution named in the display name",
    ),
    (
        "tenant_cluster",
        "One of many registrations that differ only by tenant",
    ),
    ("registrant", "Who registered it"),
    ("description", "What the name and description say"),
    ("general_knowledge", "Widely known facts about the product"),
];

/// What the metadata says that bears on who can use a service.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Signals {
    /// Category keys that lean multilateral: Research & Scholarship, the Code of
    /// Conduct, and the REFEDS Anonymous, Pseudonymous, and Personalized Access
    /// categories.
    pub open_categories: Vec<String>,
    /// In eduGAIN's aggregate, so identity providers in other federations see it.
    pub in_edugain: bool,
    /// A discovery-response or request-initiator endpoint: the service expects
    /// users to pick among many identity providers.
    pub discovery: bool,
    /// An institution named in the display name ("University of Alaska" in "Zoom -
    /// University of Alaska Statewide System").
    pub institution_named: Option<String>,
    /// One of many registrations that differ only by tenant.
    pub tenant_cluster: Option<Cluster>,
    /// The registrant organization.
    pub registrant: Option<String>,
}

/// A service's sharing label and what decided it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SharingReading {
    pub value: Sharing,
    /// `metadata` (a rule over the signals) or `generated` (Claude's reading);
    /// absent when neither decided.
    pub source: Option<String>,
    /// Why, in one sentence.
    pub reason: Option<String>,
    /// For a generated reading: the model, when, and the facts it relied on.
    pub model: Option<String>,
    pub generated_at: Option<String>,
    pub relied_on: Vec<String>,
    pub signals: Signals,
}

/// The organization a service is registered under, as compared across a cluster.
fn registrant(s: &Service) -> Option<String> {
    pick(&s.organization.display_names, Some("en"))
        .or_else(|| pick(&s.organization.names, Some("en")))
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
}

/// `https://*.zoom.us`, for `https://alaska.zoom.us`: the entityID with the host's
/// first label replaced, when the host has at least three labels. Lowercase.
pub fn host_pattern(entity_id: &str) -> Option<String> {
    let (scheme, rest) = entity_id.split_once("://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..end];
    let (_, parent) = host.split_once('.')?;
    parent
        .contains('.')
        .then(|| format!("{scheme}://*.{parent}{}", &rest[end..]).to_lowercase())
}

/// Tenant clusters, by member entityID: five or more registrations that share a
/// [`host_pattern`], at least 80% of them under one registrant. A pattern spread
/// over many registrants (`*.carsi.edu.cn`, a federation's hosting space for many
/// providers) is not one.
pub fn clusters(services: &[Service]) -> HashMap<String, Cluster> {
    let mut groups: BTreeMap<String, Vec<&Service>> = BTreeMap::new();
    for s in services {
        if let Some(p) = host_pattern(&s.entity_id) {
            groups.entry(p).or_default().push(s);
        }
    }
    let mut out = HashMap::new();
    for (pattern, members) in groups {
        if members.len() < CLUSTER_MIN {
            continue;
        }
        let mut by: HashMap<String, usize> = HashMap::new();
        for s in &members {
            *by.entry(registrant(s).unwrap_or_default().to_lowercase())
                .or_default() += 1;
        }
        let (top, n) = by
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
            .unwrap_or_default();
        if (n as f64) < CLUSTER_SAME_REGISTRANT * members.len() as f64 {
            continue;
        }
        let shown = members
            .iter()
            .find_map(|s| registrant(s).filter(|r| r.to_lowercase() == top))
            .unwrap_or(top);
        let cluster = Cluster {
            pattern: pattern.clone(),
            size: members.len(),
            registrant: shown,
        };
        for s in members {
            out.insert(s.entity_id.clone(), cluster.clone());
        }
    }
    out
}

pub fn signals(s: &Service, cluster: Option<&Cluster>) -> Signals {
    Signals {
        open_categories: s
            .categories
            .iter()
            .filter(|c| OPEN_CATEGORIES.contains(&c.as_str()))
            .cloned()
            .collect(),
        in_edugain: s.sources.contains(&Source::Edugain),
        discovery: s.supports_discovery_response || s.supports_request_initiator,
        institution_named: pick(&s.display_names, Some("en")).and_then(institution_in),
        tenant_cluster: cluster.cloned(),
        registrant: registrant(s),
    }
}

/// Rule 1: a tenant cluster that nothing contradicts.
pub fn bilateral_rule(sig: &Signals) -> Option<String> {
    let c = sig.tenant_cluster.as_ref()?;
    (!sig.in_edugain && sig.open_categories.is_empty()).then(|| {
        format!(
            "One of {} registrations at {} that differ only by tenant, mostly by {}, and not published to eduGAIN.",
            c.size, c.pattern, c.registrant
        )
    })
}

/// Rule 3: an open category, eduGAIN, and no tenant cluster.
pub fn multilateral_rule(sig: &Signals) -> Option<String> {
    if sig.tenant_cluster.is_some() || !sig.in_edugain {
        return None;
    }
    let key = sig
        .open_categories
        .iter()
        .find(|c| DECIDING_CATEGORIES.contains(&c.as_str()))?;
    let label = categories::get(key).map(|d| d.label).unwrap_or(key);
    Some(format!(
        "Carries the {label} category, which is for services that take users from any identity provider, and is published to eduGAIN."
    ))
}

/// The rules and Claude's reading, in order (see the module documentation).
pub fn classify(s: &Service, cluster: Option<&Cluster>) -> SharingReading {
    let signals = signals(s, cluster);
    let from_rule = |value, reason: String, signals: Signals| SharingReading {
        value,
        source: Some("metadata".into()),
        reason: Some(reason),
        model: None,
        generated_at: None,
        relied_on: Vec::new(),
        signals,
    };
    if let Some(reason) = bilateral_rule(&signals) {
        return from_rule(Sharing::Bilateral, reason, signals);
    }
    if let Some((value, l)) = s
        .sharing
        .as_ref()
        .and_then(|l| Some((Sharing::from_key(&l.label)?, l)))
        .filter(|(v, _)| *v != Sharing::Unknown)
    {
        return SharingReading {
            value,
            source: Some("generated".into()),
            reason: Some(l.reason.clone()).filter(|r| !r.is_empty()),
            model: Some(l.model.clone()),
            generated_at: Some(l.generated_at.clone()),
            relied_on: l.relied_on.clone(),
            signals,
        };
    }
    if let Some(reason) = multilateral_rule(&signals) {
        return from_rule(Sharing::Multilateral, reason, signals);
    }
    SharingReading {
        value: Sharing::Unknown,
        source: None,
        reason: None,
        model: None,
        generated_at: None,
        relied_on: Vec::new(),
        signals,
    }
}

/// Words that name an institution of higher education, in the languages the
/// catalog's names use. Matched at the start of a word, case-insensitively.
const INSTITUTION_WORDS: &[&str] = &[
    "univers", // University, Universität, Université, Universidad, Università, Universiteit
    "college",
    "hochschule",
    "polytechnic",
    "politecnico",
];
/// Short words that join an institution's name: "University *of* Alaska".
const NAME_JOINS: &[&str] = &[
    "of", "at", "in", "and", "&", "de", "du", "des", "la", "di", "del", "für", "the",
];

/// The institution a display name names, such as "University of Alaska" in "Zoom -
/// University of Alaska Statewide System": the run of capitalized words around an
/// institution word.
pub fn institution_in(name: &str) -> Option<String> {
    // Separators end a name: "Zoom - University of Alaska", "Baylor University - Hosted Alma".
    name.replace(" - ", "|")
        .split(['|', '–', '—', '(', ')', ',', ':', '/'])
        .find_map(institution_in_segment)
}

fn institution_in_segment(segment: &str) -> Option<String> {
    let words: Vec<&str> = segment.split_whitespace().collect();
    let at = words.iter().position(|w| {
        let lc = w.to_lowercase();
        INSTITUTION_WORDS.iter().any(|k| lc.starts_with(k))
    })?;
    let part =
        |w: &str| w.chars().next().is_some_and(char::is_uppercase) || NAME_JOINS.contains(&w);
    // Joining words only on the right: "Canvas at the University of Michigan".
    let capital = |w: &str| w.chars().next().is_some_and(char::is_uppercase);
    let mut start = at;
    while start > 0 && capital(words[start - 1]) {
        start -= 1;
    }
    let mut end = at + 1;
    while end < words.len() && part(words[end]) {
        end += 1;
    }
    while end > at + 1 && NAME_JOINS.contains(&words[end - 1]) {
        end -= 1;
    }
    Some(words[start..end].join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SharingLabel;

    fn svc(entity_id: &str, org: &str) -> Service {
        let mut s = Service {
            entity_id: entity_id.into(),
            sources: vec![Source::Incommon],
            ..Default::default()
        };
        s.organization.display_names.insert("en".into(), org.into());
        s
    }

    #[test]
    fn host_patterns() {
        assert_eq!(
            host_pattern("https://alaska.zoom.us").as_deref(),
            Some("https://*.zoom.us")
        );
        assert_eq!(
            host_pattern("http://Auburn.Instructure.com/saml2").as_deref(),
            Some("http://*.instructure.com/saml2")
        );
        assert_eq!(host_pattern("https://zoom.us/sp"), None);
        assert_eq!(host_pattern("urn:mace:example"), None);
    }

    #[test]
    fn clusters_need_five_registrations_mostly_by_one_registrant() {
        let mut services: Vec<Service> = (0..5)
            .map(|i| {
                svc(
                    &format!("https://campus{i}.zoom.us"),
                    "Zoom Video Communications, Inc.",
                )
            })
            .collect();
        // Four of a pattern is not a cluster.
        services
            .extend((0..4).map(|i| svc(&format!("https://c{i}.qualtrics.com/sp"), "Qualtrics")));
        // A federation's hosting space for many providers is not one either.
        services.extend((0..6).map(|i| {
            svc(
                &format!("https://sp-{i}.carsi.edu.cn/shibboleth"),
                &format!("Vendor {i}"),
            )
        }));
        let c = clusters(&services);
        assert_eq!(c.len(), 5);
        let zoom = &c["https://campus3.zoom.us"];
        assert_eq!(zoom.pattern, "https://*.zoom.us");
        assert_eq!(
            (zoom.size, zoom.registrant.as_str()),
            (5, "Zoom Video Communications, Inc.")
        );

        // One outlier in five is still a cluster (80%); two in six is not.
        services[0]
            .organization
            .display_names
            .insert("en".into(), "Someone Else".into());
        assert_eq!(clusters(&services).len(), 5);
        services.push(svc("https://campus5.zoom.us", "A Third"));
        assert_eq!(clusters(&services).len(), 0);
    }

    fn reading(s: &Service, cluster: bool) -> SharingReading {
        let c = Cluster {
            pattern: "https://*.zoom.us".into(),
            size: 226,
            registrant: "Zoom Video Communications, Inc.".into(),
        };
        classify(s, cluster.then_some(&c))
    }

    #[test]
    fn a_tenant_cluster_is_bilateral_unless_something_contradicts_it() {
        let s = svc("https://alaska.zoom.us", "Zoom");
        let r = reading(&s, true);
        assert_eq!(
            (r.value, r.source.as_deref()),
            (Sharing::Bilateral, Some("metadata"))
        );
        assert!(r
            .reason
            .unwrap()
            .contains("226 registrations at https://*.zoom.us"));

        // Published to eduGAIN, or carrying an open category: Claude decides.
        let mut s2 = s.clone();
        s2.sources.push(Source::Edugain);
        assert_eq!(reading(&s2, true).value, Sharing::Unknown);
        let mut s3 = s.clone();
        s3.categories.push("code-of-conduct-v2".into());
        assert_eq!(reading(&s3, true).value, Sharing::Unknown);

        // The rule outranks Claude's reading.
        let mut s4 = s.clone();
        s4.sharing = Some(SharingLabel {
            label: "multilateral".into(),
            ..Default::default()
        });
        assert_eq!(reading(&s4, true).value, Sharing::Bilateral);
    }

    #[test]
    fn claude_then_open_categories() {
        let mut s = svc("https://login.at.internet2.edu/sp", "Internet2");
        s.sources.push(Source::Edugain);
        s.categories = vec!["research-and-scholarship".into(), "sirtfi".into()];
        let r = reading(&s, false);
        assert_eq!(
            (r.value, r.source.as_deref()),
            (Sharing::Multilateral, Some("metadata"))
        );
        assert!(r.reason.unwrap().contains("Research & Scholarship"));
        assert_eq!(r.signals.open_categories, ["research-and-scholarship"]);

        // Claude's reading comes first.
        s.sharing = Some(SharingLabel {
            label: "bilateral".into(),
            reason: "Internal to Internet2.".into(),
            relied_on: vec!["description".into()],
            model: "m".into(),
            generated_at: "2026-10-29T00:00:00Z".into(),
            cluster: None,
        });
        let r = reading(&s, false);
        assert_eq!(
            (r.value, r.source.as_deref()),
            (Sharing::Bilateral, Some("generated"))
        );
        assert_eq!(r.model.as_deref(), Some("m"));
        assert_eq!(r.relied_on, ["description"]);
        // An unknown reading leaves it to the rule.
        s.sharing.as_mut().unwrap().label = "unknown".into();
        assert_eq!(reading(&s, false).value, Sharing::Multilateral);

        // The Code of Conduct alone, or no eduGAIN, decides nothing.
        let mut t = svc("https://wiki.example.ac.uk/sp", "Example");
        t.sources.push(Source::Edugain);
        t.categories = vec!["code-of-conduct-v2".into()];
        assert_eq!(reading(&t, false).value, Sharing::Unknown);
        t.categories = vec!["research-and-scholarship".into()];
        t.sources = vec![Source::Incommon];
        let r = reading(&t, false);
        assert_eq!((r.value, r.source), (Sharing::Unknown, None));
    }

    #[test]
    fn signals_record_the_facts() {
        let mut s = svc("https://alaska.zoom.us", "Zoom Video Communications, Inc.");
        s.display_names.insert(
            "en".into(),
            "Zoom - University of Alaska Statewide System".into(),
        );
        s.supports_request_initiator = true;
        let sig = signals(&s, None);
        assert!(sig.discovery && !sig.in_edugain);
        assert_eq!(
            sig.institution_named.as_deref(),
            Some("University of Alaska Statewide System")
        );
        assert_eq!(
            sig.registrant.as_deref(),
            Some("Zoom Video Communications, Inc.")
        );
    }

    #[test]
    fn institutions_in_names() {
        for (name, want) in [
            ("Baylor University - Hosted Alma", Some("Baylor University")),
            ("AcademicWorks - Amherst College", Some("Amherst College")),
            (
                "Canvas at the University of Michigan",
                Some("University of Michigan"),
            ),
            ("Universität Münster (Staging)", Some("Universität Münster")),
            (
                "Contra Costa Community College District (Canvas)",
                Some("Contra Costa Community College District"),
            ),
            ("Zoom", None),
            ("Elsevier ScienceDirect", None),
        ] {
            assert_eq!(institution_in(name).as_deref(), want, "{name}");
        }
    }

    #[test]
    fn keys_round_trip() {
        for s in Sharing::ALL {
            assert_eq!(Sharing::from_key(s.key()), Some(s));
        }
        assert_eq!(Sharing::from_key("open"), None);
        assert_eq!(FACTS.len(), 8);
    }
}
