// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Entity categories and assurance certifications the catalog understands.
//!
//! Several categories circulate under more than one URI — http/https variants, a
//! deprecated InCommon-only Research & Scholarship URI, and Code of Conduct v2 under
//! both refeds.org and geant.net. Each [`CategoryDef`] is therefore a *set* of
//! synonyms, and matching is always on whole values: `refeds.org/sirtfi` is a prefix
//! of `refeds.org/sirtfi2`, so substring matching would be wrong.

use serde::Serialize;
use utoipa::ToSchema;

use crate::ns;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CategoryKind {
    EntityCategory,
    AssuranceCertification,
    FederationTag,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CategoryDef {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: CategoryKind,
    pub uris: &'static [&'static str],
    pub spec_url: Option<&'static str>,
    pub description: &'static str,
}

pub const DEPRECATED_INCOMMON_RS: &[&str] = &[
    "http://id.incommon.org/category/research-and-scholarship",
    "https://id.incommon.org/category/research-and-scholarship",
];

pub static CATEGORIES: &[CategoryDef] = &[
    CategoryDef {
        key: "research-and-scholarship",
        label: "Research & Scholarship",
        kind: CategoryKind::EntityCategory,
        uris: &[
            "http://refeds.org/category/research-and-scholarship",
            "https://refeds.org/category/research-and-scholarship",
            "http://id.incommon.org/category/research-and-scholarship",
            "https://id.incommon.org/category/research-and-scholarship",
        ],
        spec_url: Some("https://refeds.org/category/research-and-scholarship"),
        description: "Service supports research and scholarship; for SPs this tag is reviewed and assigned by the registering federation.",
    },
    CategoryDef {
        key: "anonymous",
        label: "Anonymous Access",
        kind: CategoryKind::EntityCategory,
        uris: &["https://refeds.org/category/anonymous"],
        spec_url: Some("https://refeds.org/category/anonymous"),
        description: "Service needs only proof of successful authentication and organizational affiliation.",
    },
    CategoryDef {
        key: "pseudonymous",
        label: "Pseudonymous Access",
        kind: CategoryKind::EntityCategory,
        uris: &["https://refeds.org/category/pseudonymous"],
        spec_url: Some("https://refeds.org/category/pseudonymous"),
        description: "Service needs a pairwise identifier and affiliation, but not personal identity.",
    },
    CategoryDef {
        key: "personalized",
        label: "Personalized Access",
        kind: CategoryKind::EntityCategory,
        uris: &["https://refeds.org/category/personalized"],
        spec_url: Some("https://refeds.org/category/personalized"),
        description: "Service needs a persistent identifier, name, and email address.",
    },
    CategoryDef {
        key: "code-of-conduct-v2",
        label: "Data Protection Code of Conduct v2",
        kind: CategoryKind::EntityCategory,
        uris: &[
            "https://refeds.org/category/code-of-conduct/v2",
            "http://www.geant.net/uri/dataprotection-code-of-conduct/v2",
            "https://www.geant.net/uri/dataprotection-code-of-conduct/v2",
        ],
        spec_url: Some("https://refeds.org/category/code-of-conduct/v2"),
        description: "Service operator commits to the REFEDS Data Protection Code of Conduct.",
    },
    CategoryDef {
        key: "code-of-conduct-v1",
        label: "GÉANT Code of Conduct v1",
        kind: CategoryKind::EntityCategory,
        uris: &[
            "http://www.geant.net/uri/dataprotection-code-of-conduct/v1",
            "https://www.geant.net/uri/dataprotection-code-of-conduct/v1",
        ],
        spec_url: None,
        description: "Service operator commits to the GÉANT Data Protection Code of Conduct, version 1.",
    },
    CategoryDef {
        key: "hide-from-discovery",
        label: "Hide From Discovery",
        kind: CategoryKind::EntityCategory,
        uris: &["http://refeds.org/category/hide-from-discovery"],
        spec_url: Some("http://refeds.org/category/hide-from-discovery"),
        description: "Entity asks discovery services not to list it.",
    },
    CategoryDef {
        key: "sirtfi",
        label: "Sirtfi",
        kind: CategoryKind::AssuranceCertification,
        uris: &["https://refeds.org/sirtfi"],
        spec_url: Some("https://refeds.org/sirtfi"),
        description: "Operator asserts the Security Incident Response Trust Framework for Federated Identity (v1).",
    },
    CategoryDef {
        key: "sirtfi2",
        label: "Sirtfi v2",
        kind: CategoryKind::AssuranceCertification,
        uris: &["https://refeds.org/sirtfi2"],
        spec_url: Some("https://refeds.org/sirtfi2"),
        description: "Operator asserts Sirtfi version 2.",
    },
    CategoryDef {
        key: "registered-by-incommon",
        label: "Registered by InCommon",
        kind: CategoryKind::FederationTag,
        uris: &["http://id.incommon.org/category/registered-by-incommon"],
        spec_url: None,
        description: "InCommon stamps this on every entity it registers.",
    },
];

/// REFEDS authentication profiles. These describe a single authentication event and
/// are signaled at runtime (`AuthnContextClassRef`), so finding one filed as an entity
/// category is a misfiling worth noting.
pub const AUTHN_PROFILES: &[&str] = &[
    "https://refeds.org/profile/mfa",
    "https://refeds.org/profile/mfa/phr",
    "https://refeds.org/profile/sfa",
];

pub fn get(key: &str) -> Option<&'static CategoryDef> {
    CATEGORIES.iter().find(|c| c.key == key)
}

/// Canonical keys for a service's tags. Entity-category definitions match only the
/// `entity-category` attribute; assurance definitions only `assurance-certification`.
pub fn classify(entity_categories: &[String], assurance: &[String]) -> Vec<String> {
    let mut keys = Vec::new();
    for def in CATEGORIES {
        let pool = match def.kind {
            CategoryKind::AssuranceCertification => assurance,
            CategoryKind::EntityCategory | CategoryKind::FederationTag => entity_categories,
        };
        if pool.iter().any(|v| def.uris.contains(&v.as_str())) {
            keys.push(def.key.to_owned());
        }
    }
    keys
}

/// The attribute names [`classify`] consumes; anything else lands in
/// `other_entity_attributes`.
pub fn is_known_attribute(name: &str) -> bool {
    matches!(
        name,
        ns::ATTR_ENTITY_CATEGORY
            | ns::ATTR_ENTITY_CATEGORY_SUPPORT
            | ns::ATTR_ASSURANCE_CERTIFICATION
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sirtfi_is_not_a_prefix_match() {
        let keys = classify(&[], &["https://refeds.org/sirtfi2".into()]);
        assert_eq!(keys, vec!["sirtfi2"]);
    }

    #[test]
    fn deprecated_rs_counts_as_rs() {
        let keys = classify(&[DEPRECATED_INCOMMON_RS[0].into()], &[]);
        assert_eq!(keys, vec!["research-and-scholarship"]);
    }
}
