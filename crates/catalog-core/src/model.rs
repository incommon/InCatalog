// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! The catalog's data model and the on-disk snapshot format.
//!
//! Fields are split by provenance: everything on [`Service`] except `curated` is
//! derived from signed federation metadata; `curated` is local editorial overlay and
//! is never mixed into the metadata-derived fields.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Values keyed by `xml:lang`; the empty key holds values that carried no language.
pub type LangMap = BTreeMap<String, String>;

/// Which aggregate an entity was seen in.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// `https://mdq.incommon.org/entities` — InCommon registrations plus InCommon's
    /// filtered import of eduGAIN.
    Incommon,
    /// `https://mds.edugain.org/edugain-v2.xml` — everything every member
    /// federation exports to eduGAIN.
    Edugain,
}

/// Where a service can be used from, derived from its registrar and which
/// aggregates carry it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Availability {
    /// Registered by InCommon and not exported to eduGAIN.
    IncommonOnly,
    /// Registered by InCommon and exported to eduGAIN.
    IncommonAndEdugain,
    /// Registered by another federation; present in InCommon's eduGAIN import.
    EdugainImportedByIncommon,
    /// Registered by another federation; in eduGAIN but not in InCommon's import.
    EdugainOnly,
    /// Registered by another federation; in InCommon's aggregate but not eduGAIN's.
    IncommonAggregateOnly,
}

impl Availability {
    pub const ALL: [Availability; 5] = [
        Availability::IncommonOnly,
        Availability::IncommonAndEdugain,
        Availability::EdugainImportedByIncommon,
        Availability::EdugainOnly,
        Availability::IncommonAggregateOnly,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Availability::IncommonOnly => "incommon-only",
            Availability::IncommonAndEdugain => "incommon-and-edugain",
            Availability::EdugainImportedByIncommon => "edugain-imported-by-incommon",
            Availability::EdugainOnly => "edugain-only",
            Availability::IncommonAggregateOnly => "incommon-aggregate-only",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Availability::IncommonOnly => "InCommon only",
            Availability::IncommonAndEdugain => "InCommon, exported to eduGAIN",
            Availability::EdugainImportedByIncommon => "eduGAIN, visible in InCommon",
            Availability::EdugainOnly => "eduGAIN, not in InCommon's import",
            Availability::IncommonAggregateOnly => "In InCommon's aggregate only",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.key() == k)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct Logo {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub lang: Option<String>,
    /// Ingest fetched this URL and it did not return an image (404, DNS failure,
    /// HTML error page). Kept, so the metadata stays as published, but never shown.
    #[serde(default)]
    pub broken: bool,
}

/// A plain-language summary and search keywords written by Claude from this
/// entry's metadata during ingest. Generated, not published by the registrant:
/// stored apart from the metadata fields and always shown as generated.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct Enrichment {
    /// One to three sentences on what the service is and who offers it. Empty when
    /// the metadata says too little (`basis` is then `insufficient`).
    pub summary: String,
    /// Lowercase terms people might search for, including synonyms and the kind
    /// of service ("lms", "course management").
    pub keywords: Vec<String>,
    /// `production`, `test`, `staging`, `development`, or `unknown`, read from names,
    /// hosts, and descriptions.
    pub environment: String,
    /// `metadata` (only what the metadata says), `metadata_and_general_knowledge`
    /// (also widely known facts about a product the metadata names), or
    /// `insufficient`.
    pub basis: String,
    pub model: String,
    pub generated_at: String,
}

/// A logo found on the organization's website because the metadata had no working
/// one. Not part of the metadata; recorded separately with where it came from.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct DiscoveredLogo {
    pub url: String,
    /// The page the logo was found on.
    pub page: String,
    /// The model that chose it among the page's candidate images.
    pub chosen_by: String,
    pub found_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct Organization {
    pub names: LangMap,
    pub display_names: LangMap,
    pub urls: LangMap,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct Contact {
    /// `technical`, `support`, `administrative`, `billing`, `other`, or `security`
    /// (the REFEDS security contact, folded in from `remd:contactType`).
    pub kind: String,
    pub emails: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct Registration {
    /// `mdrpi:RegistrationInfo/@registrationAuthority` — the federation that
    /// registered the entity and vouches for it.
    pub authority: Option<String>,
    pub instant: Option<String>,
    pub policies: LangMap,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct EntityAttribute {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct RequestedAttribute {
    pub name: String,
    pub friendly_name: Option<String>,
    pub name_format: Option<String>,
    pub required: bool,
}

/// Editorial overlay, maintained outside federation metadata. See [`crate::curation`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct Curated {
    /// e.g. `commercial`, `non-commercial`, `government`. Free text by design.
    pub provider_type: Option<String>,
    pub tags: Vec<String>,
    pub summary: Option<String>,
    pub hidden: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct Service {
    /// Lowercase hex SHA-1 of `entity_id` (the MDQ `{sha1}` identifier).
    pub id: String,
    pub entity_id: String,

    /// `mdui:DisplayName`.
    pub display_names: LangMap,
    /// `mdui:Description`.
    pub descriptions: LangMap,
    pub information_urls: LangMap,
    pub privacy_statement_urls: LangMap,
    pub keywords: Vec<String>,
    pub logos: Vec<Logo>,
    /// `md:AttributeConsumingService/md:ServiceName`, a fallback when MDUI is absent.
    pub service_names: LangMap,
    pub service_descriptions: LangMap,

    pub organization: Organization,
    pub contacts: Vec<Contact>,
    pub registration: Registration,

    /// Raw values of `http://macedir.org/entity-category` ("this entity *is* in X").
    pub entity_categories: Vec<String>,
    /// Raw values of `http://macedir.org/entity-category-support`.
    pub entity_category_support: Vec<String>,
    pub assurance_certifications: Vec<String>,
    pub other_entity_attributes: Vec<EntityAttribute>,
    /// Canonical category keys from [`crate::categories`], synonyms collapsed.
    pub categories: Vec<String>,

    pub requested_attributes: Vec<RequestedAttribute>,
    pub protocols: Vec<String>,
    pub name_id_formats: Vec<String>,
    pub acs_bindings: Vec<String>,
    pub has_signing_key: bool,
    pub has_encryption_key: bool,
    pub supports_discovery_response: bool,

    pub sources: Vec<Source>,
    /// Other registration authorities publishing this same entityID (a clash).
    pub also_registered_by: Vec<String>,
    pub availability: Option<Availability>,
    /// eduGAIN federation code for the registrar, when eduGAIN lists it.
    pub federation: Option<String>,
    /// Factual observations about this entity's metadata (not judgments about its operator).
    pub notes: Vec<String>,

    pub curated: Option<Curated>,

    /// Set by ingest only when no metadata logo works. See [`DiscoveredLogo`].
    #[serde(default)]
    pub discovered_logo: Option<DiscoveredLogo>,

    /// Generated summary and keywords. See [`Enrichment`].
    #[serde(default)]
    pub enrichment: Option<Enrichment>,
}

impl Service {
    pub fn registrar(&self) -> Option<&str> {
        self.registration.authority.as_deref()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct FederationStats {
    pub entities: Option<u64>,
    pub idps: Option<u64>,
    pub sps: Option<u64>,
}

/// A federation, as described by the eduGAIN database API (`list_feds_full`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct Federation {
    pub code: String,
    pub name: String,
    pub registration_authority: String,
    /// `production`, `candidate`, `suspended`, or the raw status code.
    pub status: String,
    pub country_codes: Vec<String>,
    pub countries: Vec<String>,
    pub url: Option<String>,
    pub metadata_url: Option<String>,
    pub contact_email: Option<String>,
    pub security_contact_email: Option<String>,
    pub registration_practice_urls: Vec<String>,
    pub policy_urls: Vec<String>,
    pub membership_date: Option<String>,
    pub production_date: Option<String>,
    /// Counts as reported by eduGAIN for this federation's export.
    pub edugain_reported: FederationStats,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct SourceInfo {
    pub source: Option<Source>,
    pub url: String,
    pub fetched_at: String,
    pub last_modified: Option<String>,
    pub etag: Option<String>,
    pub sha256: Option<String>,
    pub bytes: u64,
    /// `EntitiesDescriptor/@Name`.
    pub name: Option<String>,
    /// `EntitiesDescriptor/@validUntil`.
    pub valid_until: Option<String>,
    pub entities: usize,
    pub service_providers: usize,
    pub identity_providers: usize,
    /// True only when the aggregate's XML signature was checked against a pinned
    /// federation signing certificate during ingest.
    pub signature_verified: bool,
}

/// Everything the API serves, produced by `catalog-ingest`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema_version: u32,
    pub generated_at: String,
    pub generator: String,
    pub sources: Vec<SourceInfo>,
    pub federations: Vec<Federation>,
    pub services: Vec<Service>,
    /// Identity providers per registration authority, across both aggregates.
    pub idp_counts: BTreeMap<String, usize>,
    pub warnings: Vec<String>,
}

/// Pick the best value from a language map: the requested language, then English,
/// then untagged, then anything.
pub fn pick<'a>(map: &'a LangMap, lang: Option<&str>) -> Option<&'a str> {
    let try_key = |k: &str| map.get(k).map(String::as_str).filter(|s| !s.is_empty());
    lang.and_then(try_key)
        .or_else(|| try_key("en"))
        .or_else(|| try_key(""))
        .or_else(|| map.values().map(String::as_str).find(|s| !s.is_empty()))
}
