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

/// Whether a service is open to users from any institution, as Claude read it
/// during ingest from the metadata and the signals in [`crate::sharing`].
/// Generated, like [`Enrichment`]: stored apart and always shown as generated.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct SharingLabel {
    /// `multilateral`, `bilateral`, or `unknown`.
    pub label: String,
    /// One sentence on why.
    pub reason: String,
    /// Which of the facts it was given it relied on (see [`crate::sharing::FACTS`]).
    pub relied_on: Vec<String>,
    /// Classified once for a whole tenant cluster, by its pattern.
    #[serde(default)]
    pub cluster: Option<String>,
    pub model: String,
    pub generated_at: String,
}

/// The sectors of a service's organization, as Claude read them during ingest from
/// the organization's name, website, and services. Generated, like
/// [`Enrichment`]: stored apart and always shown as generated. See
/// [`crate::sector`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct SectorLabel {
    /// Keys from [`crate::sector::Sector`]; empty when Claude could not tell.
    pub sectors: Vec<String>,
    /// One sentence on why.
    pub reason: String,
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

/// Why InCommon's eduGAIN import left this service out, as InCommon's own import
/// filter report states it. Reported by InCommon, not judged here. See
/// [`crate::import_filter`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct ImportFilter {
    /// The report this came from.
    pub report_url: String,
    /// When InCommon wrote the report.
    pub reported_at: String,
    pub findings: Vec<FilterFinding>,
}

/// One check the service failed in InCommon's import filter.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct FilterFinding {
    /// The policy rule it falls under: a key from
    /// [`crate::import_filter::Reason`], such as `saml1` or `sirtfi`.
    pub reason: String,
    /// The check as the report names it, such as `check_saml1`.
    pub check: String,
    /// The report's message, as written.
    pub message: String,
}

/// An entry on one of OFAC's sanctions lists that this service's organization name or
/// domain matched. A possible match is a name collision until the curation overlay
/// confirms it; the catalog never states that a provider is sanctioned. See
/// [`crate::ofac`].
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OfacMatch {
    /// `possible` until the curation overlay confirms it.
    pub status: crate::ofac::MatchStatus,
    /// `sdn` or `consolidated`.
    pub list: crate::ofac::List,
    /// The entry's UID on that list.
    pub uid: String,
    /// The entry's name, as OFAC lists it.
    pub name: String,
    /// OFAC's program tags for the entry, such as `CUBA` or `CMIC-EO13959`.
    pub programs: Vec<String>,
    /// What matched.
    pub matched_on: Vec<MatchBasis>,
    /// When OFAC published the list this was read from.
    pub list_published: Option<String>,
    /// When the curation overlay confirmed it.
    pub decided: Option<String>,
    /// The curators' note on it.
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct MatchBasis {
    /// `name` (the organization's name equals the entry's name or a strong a.k.a.)
    /// or `domain` (a host of the service is a website the entry lists, or under it).
    pub on: String,
    /// The service's side: its organization name, or the host.
    pub ours: String,
    /// The list's side: the name or website domain as OFAC lists it.
    pub theirs: String,
}

/// OFAC's lists as a snapshot used them, and what they matched.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct OfacReport {
    pub lists: Vec<OfacListInfo>,
    /// Services whose strongest match is possible.
    pub possible: usize,
    /// Services with a match the curation overlay confirmed.
    pub confirmed: usize,
    /// Matches the curation overlay dismissed as name collisions.
    pub dismissed: usize,
    /// Providers (by organization name) with a possible or confirmed match.
    pub providers: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct OfacListInfo {
    /// `sdn` or `consolidated`.
    pub list: String,
    pub url: String,
    /// When OFAC published it.
    pub published: Option<String>,
    pub fetched_at: String,
    /// Entries for entities, the only ones compared.
    pub entities: usize,
}

/// Whether a service still answers, from ingest's own checks of its hosts. See
/// [`crate::health`].
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    /// On a recent check, a login host resolved, connected and passed TLS.
    Alive,
    /// Every login host has failed several checks in a row over several days.
    Dead,
    /// Not checked yet, or no clear answer yet.
    #[default]
    Unknown,
}

impl HealthStatus {
    pub const ALL: [HealthStatus; 3] = [
        HealthStatus::Alive,
        HealthStatus::Unknown,
        HealthStatus::Dead,
    ];

    pub fn key(self) -> &'static str {
        match self {
            HealthStatus::Alive => "alive",
            HealthStatus::Dead => "dead",
            HealthStatus::Unknown => "unknown",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            HealthStatus::Alive => "Reachable",
            HealthStatus::Dead => "Unreachable from InCatalog",
            HealthStatus::Unknown => "Not checked, or no clear answer",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == k)
    }
}

/// What ingest's checks found for a service. Observed from outside, not reported by
/// the registrant.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct Health {
    pub status: HealthStatus,
    /// When `dead`: the first failed check of the run of failures.
    pub since: Option<String>,
    /// The last time a login host answered.
    pub last_ok: Option<String>,
    /// Each login host (`endpoint_hosts`) and what its checks found.
    pub hosts: Vec<HostHealth>,
    /// Information, organization and privacy pages that answered 404 or 410. A broken
    /// page doesn't make a service dead.
    pub broken_links: Vec<BrokenLink>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct HostHealth {
    pub host: String,
    pub status: HealthStatus,
    /// What the latest check found when it failed: `dns` (the name doesn't exist),
    /// `connect` (refused), `tls_expired`, `tls_name` (certificate for another
    /// name), `tls_untrusted`, `tls`, or for no clear answer `dns_transient`,
    /// `private_address` (resolves only inside a network), `timeout`, `other`.
    pub failure: Option<String>,
    pub detail: Option<String>,
    pub checked_at: Option<String>,
    pub last_ok: Option<String>,
    /// The first failed check of the current run of failures.
    pub first_failed: Option<String>,
    pub failures_in_a_row: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct BrokenLink {
    pub url: String,
    /// `information`, `organization`, or `privacy`.
    pub kind: String,
    /// Such as `HTTP 404`.
    pub detail: String,
    /// The first check that found it broken.
    pub since: String,
}

/// The import filter report a snapshot used.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct FilterReportInfo {
    pub url: String,
    /// When InCommon wrote it.
    pub reported_at: String,
    pub fetched_at: String,
    /// Entities it marks as removed, identity providers included.
    pub entities: usize,
    /// `edugain-only` services it gives a reason for.
    pub services_with_reason: usize,
    /// `edugain-only` services it doesn't mention: out of InCommon's import for some
    /// other reason, such as the aggregates being published at different times.
    pub services_without_reason: usize,
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

/// A metadata endpoint's `Location` and `Binding`, as published.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Endpoint {
    pub location: String,
    pub binding: String,
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
    /// An `init:RequestInitiator` endpoint, where any IdP's user can start a login.
    #[serde(default)]
    pub supports_request_initiator: bool,
    /// The `init:RequestInitiator` endpoints, as published. See [`crate::login`].
    #[serde(default)]
    pub request_initiators: Vec<Endpoint>,
    /// The `idpdisc:DiscoveryResponse` endpoints, where a discovery service returns the
    /// user's choice of institution: the default first, then by `index`. See
    /// [`crate::login`].
    #[serde(default)]
    pub discovery_responses: Vec<Endpoint>,
    /// Hosts of the `AssertionConsumerService` locations, lowercase, without repeats.
    #[serde(default)]
    pub endpoint_hosts: Vec<String>,

    pub sources: Vec<Source>,
    /// Other registration authorities publishing this same entityID (a clash).
    pub also_registered_by: Vec<String>,
    pub availability: Option<Availability>,
    /// Why InCommon's import filtered it out, when it is `edugain-only` and
    /// InCommon's report says. See [`ImportFilter`].
    #[serde(default)]
    pub import_filter: Option<ImportFilter>,
    /// Entries on OFAC's sanctions lists its organization name or domain matched,
    /// confirmed first. Empty for almost every service. See [`OfacMatch`].
    #[serde(default)]
    pub ofac: Vec<OfacMatch>,
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

    /// Generated multilateral/bilateral label. See [`SharingLabel`].
    #[serde(default)]
    pub sharing: Option<SharingLabel>,

    /// Generated sector of the organization. See [`SectorLabel`].
    #[serde(default)]
    pub sector: Option<SectorLabel>,

    /// Whether it still answers, from ingest's checks. See [`Health`].
    #[serde(default)]
    pub health: Option<Health>,
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
    /// InCommon's import filter report, when ingest could read it.
    #[serde(default)]
    pub import_filter_report: Option<FilterReportInfo>,
    /// OFAC's sanctions lists, when ingest could read them.
    #[serde(default)]
    pub ofac: Option<OfacReport>,
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
