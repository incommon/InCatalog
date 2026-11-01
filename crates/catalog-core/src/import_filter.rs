// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Why InCommon's eduGAIN import left a service out, from InCommon's own report.
//!
//! InCommon imports eduGAIN through a filter, documented in its Interfederation
//! Technical Policy ([`POLICY_URL`]). Every entity the filter removes for failing a
//! check is logged, and the logs are published about hourly as
//! `md_filter_report_<unix time>.log` beside [`REPORT_INDEX_URL`]. The only entities
//! it drops without a log line are ones InCommon registered itself and ones already
//! in its aggregate, and both of those are in InCommon's aggregate anyway. So the
//! newest log should explain every `edugain-only` service; one it doesn't is out of
//! InCommon's import for some other reason, such as the two aggregates being
//! published at different times.
//!
//! A log reads, per entity:
//!
//! ```text
//! [java] ERROR - Item https://sp.example.org/shibboleth (https://fed.example) was marked with the following Error status messages
//! [java] ERROR -     check_saml1: no POST support on SAML 1.1 SP
//! [java] ERROR -     check_sirtfi: SIRTFI requires a REFEDS security contact
//! ```
//!
//! The reasons are InCommon's, reported as it states them. [`Reason`] groups the
//! checks under the policy's own rules so they can be counted and filtered on, and
//! each finding keeps the check's name and message as the log has them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::model::{Availability, FilterFinding, ImportFilter, Service};

/// The directory listing of the published logs.
pub const REPORT_INDEX_URL: &str = "https://mdq.incommon.org/reports/md_filter_report/index.html";
/// The policy that defines the checks.
pub const POLICY_URL: &str = "https://spaces.at.internet2.edu/spaces/federation/pages/168691916/Interfederation+Technical+Policy";

/// The rule in InCommon's import policy that a failed check falls under.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    Saml1,
    Saml2,
    Https,
    RequestedAttributes,
    Bindings,
    EntityId,
    Sirtfi,
    ResearchAndScholarship,
    AlgorithmSupport,
    Mdui,
    RegistrationInfo,
    EntityAttributes,
    SamlMetadata,
    Certificate,
    Scope,
    Other,
}

impl Reason {
    pub const ALL: [Reason; 16] = [
        Reason::Saml1,
        Reason::Saml2,
        Reason::Https,
        Reason::RequestedAttributes,
        Reason::Bindings,
        Reason::EntityId,
        Reason::Sirtfi,
        Reason::ResearchAndScholarship,
        Reason::AlgorithmSupport,
        Reason::Mdui,
        Reason::RegistrationInfo,
        Reason::EntityAttributes,
        Reason::SamlMetadata,
        Reason::Certificate,
        Reason::Scope,
        Reason::Other,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Reason::Saml1 => "saml1",
            Reason::Saml2 => "saml2",
            Reason::Https => "https",
            Reason::RequestedAttributes => "requested-attributes",
            Reason::Bindings => "bindings",
            Reason::EntityId => "entity-id",
            Reason::Sirtfi => "sirtfi",
            Reason::ResearchAndScholarship => "research-and-scholarship",
            Reason::AlgorithmSupport => "algorithm-support",
            Reason::Mdui => "mdui",
            Reason::RegistrationInfo => "registration-info",
            Reason::EntityAttributes => "entity-attributes",
            Reason::SamlMetadata => "saml-metadata",
            Reason::Certificate => "certificate",
            Reason::Scope => "scope",
            Reason::Other => "other",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Reason::Saml1 => "Inconsistent SAML 1.x support",
            Reason::Saml2 => "No SAML 2 login endpoint",
            Reason::Https => "An endpoint not on HTTPS",
            Reason::RequestedAttributes => "Errors in requested attributes",
            Reason::Bindings => "Nonstandard endpoint bindings",
            Reason::EntityId => "entityID prefix not accepted",
            Reason::Sirtfi => "Doesn't meet Sirtfi",
            Reason::ResearchAndScholarship => "Doesn't meet REFEDS R&S",
            Reason::AlgorithmSupport => "Doesn't meet the algorithm-support profile",
            Reason::Mdui => "Errors in display information (MDUI)",
            Reason::RegistrationInfo => "Errors in registration information",
            Reason::EntityAttributes => "Misplaced entity attributes",
            Reason::SamlMetadata => "Doesn't meet the SAML 2.0 metadata spec",
            Reason::Certificate => "A certificate or key failed validation",
            Reason::Scope => "Faulty identity-provider scope",
            Reason::Other => "Another import check",
        }
    }

    /// The policy rule, in a sentence.
    pub fn description(self) -> &'static str {
        match self {
            Reason::Saml1 => "Entities whose metadata for SAML 1.x support is inconsistent are removed.",
            Reason::Saml2 => "Every imported service must have a SAML 2 AssertionConsumerService that supports HTTP-POST, and every identity provider a SAML 2 SingleSignOnService that supports HTTP-Redirect.",
            Reason::Https => "A service with an endpoint location that is not HTTPS-protected is removed.",
            Reason::RequestedAttributes => "Entities with errors in their RequestedAttribute elements are removed.",
            Reason::Bindings => "Entities that don't follow the standard rules for Binding values on protocol endpoints are removed.",
            Reason::EntityId => "An entityID must begin with http://, https://, or urn:mace.",
            Reason::Sirtfi => "Entities that assert Sirtfi but don't conform to the REFEDS Sirtfi specification are removed.",
            Reason::ResearchAndScholarship => "Entities that don't conform to the REFEDS Research and Scholarship entity category are removed.",
            Reason::AlgorithmSupport => "Entities that don't conform to the SAML Metadata Profile for Algorithm Support are removed.",
            Reason::Mdui => "Entities that don't conform to the SAML Metadata Extensions for Login and Discovery User Interface are removed.",
            Reason::RegistrationInfo => "Entities that don't conform to the SAML Metadata Extensions for Registration and Publication Information are removed.",
            Reason::EntityAttributes => "Entities with misplaced or duplicated EntityAttributes elements are removed.",
            Reason::SamlMetadata => "Entities that don't conform to the SAML 2.0 Metadata specification are removed.",
            Reason::Certificate => "Entities with weak keys (under 2048 bits) or certificates that fail validation are removed.",
            Reason::Scope => "An identity provider with a faulty shibmd:Scope is removed.",
            Reason::Other => "Another of the policy's checks failed; the report's message says which.",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.key() == k)
    }

    /// The rule a check named in the log falls under. A check this doesn't know
    /// is [`Reason::Other`], and keeps its own name and message.
    pub fn from_check(check: &str) -> Self {
        let base = check.split('/').next().unwrap_or(check);
        match base {
            "check_saml1" => Reason::Saml1,
            "check_sp_non_saml2" | "check_idp_non_saml2" => Reason::Saml2,
            "check_sp_tls" | "check_idp_tls" => Reason::Https,
            "check_reqattr" => Reason::RequestedAttributes,
            "check_bindings" => Reason::Bindings,
            "check_entityid_prefix" => Reason::EntityId,
            "check_sirtfi" => Reason::Sirtfi,
            "check_rands_member" | "check_rands_support" => Reason::ResearchAndScholarship,
            "check_algsupport" => Reason::AlgorithmSupport,
            "check_mdui_xslt" | "check_mdui_urls" => Reason::Mdui,
            "check_mdrpi_xslt" => Reason::RegistrationInfo,
            "check_mdattr" => Reason::EntityAttributes,
            "saml2meta" | "check_saml2meta" => Reason::SamlMetadata,
            "certificateValidation" | "check_keys" | "check_weak_keys" => Reason::Certificate,
            "checkScopes" => Reason::Scope,
            _ => Reason::Other,
        }
    }
}

/// The newest log named in the report index: its file name and Unix time.
pub fn latest_report(index_html: &str) -> Option<(String, i64)> {
    const PREFIX: &str = "md_filter_report_";
    let mut best: Option<(String, i64)> = None;
    let mut rest = index_html;
    while let Some(at) = rest.find(PREFIX) {
        rest = &rest[at + PREFIX.len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() || !rest[digits.len()..].starts_with(".log") {
            continue;
        }
        if let Ok(t) = digits.parse::<i64>() {
            if best.as_ref().is_none_or(|(_, b)| t > *b) {
                best = Some((format!("{PREFIX}{digits}.log"), t));
            }
        }
    }
    best
}

/// Each entity the log marks with Error status, and the checks it failed.
/// Warnings are left out: the policy logs them without removing the entity.
pub fn parse_report(log: &str) -> BTreeMap<String, Vec<FilterFinding>> {
    const MARKED: &str = " was marked with the following ";
    let mut out: BTreeMap<String, Vec<FilterFinding>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in log.lines() {
        let Some(at) = line.find("[java]") else {
            continue;
        };
        let body = &line[at + "[java]".len()..];
        if let Some(i) = body.find("Item ") {
            current = None;
            let item = &body[i + "Item ".len()..];
            if let Some((who, status)) = item.split_once(MARKED) {
                if status.starts_with("Error") {
                    let entity_id = who.rsplit_once(" (").map_or(who, |(e, _)| e);
                    out.entry(entity_id.to_owned()).or_default();
                    current = Some(entity_id.to_owned());
                }
            }
            continue;
        }
        let Some(entity_id) = &current else {
            continue;
        };
        let findings = out.entry(entity_id.clone()).or_default();
        match body.split_once(" - ") {
            Some((level, msg)) if level.trim() == "ERROR" => {
                let msg = msg.trim();
                let (check, message) = msg.split_once(": ").unwrap_or(("", msg));
                findings.push(FilterFinding {
                    reason: Reason::from_check(check).key().to_owned(),
                    check: check.to_owned(),
                    message: message.to_owned(),
                });
            }
            Some(_) => {}
            // A message that ran onto the next line.
            None => {
                if let Some(last) = findings.last_mut() {
                    last.message.push(' ');
                    last.message.push_str(body.trim());
                }
            }
        }
    }
    out
}

/// What [`apply`] found, for the ingest report.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Applied {
    /// `edugain-only` services the report gives a reason for.
    pub with_reason: usize,
    /// `edugain-only` services it doesn't mention.
    pub without_reason: usize,
}

/// Attach the report's findings to the `edugain-only` services it names. Other
/// services are left alone: the report's other entities are identity providers,
/// or no longer in eduGAIN.
pub fn apply(
    services: &mut [Service],
    report: &BTreeMap<String, Vec<FilterFinding>>,
    report_url: &str,
    reported_at: &str,
) -> Applied {
    let mut applied = Applied::default();
    for s in services
        .iter_mut()
        .filter(|s| s.availability == Some(Availability::EdugainOnly))
    {
        match report.get(&s.entity_id) {
            Some(findings) => {
                applied.with_reason += 1;
                s.import_filter = Some(ImportFilter {
                    report_url: report_url.to_owned(),
                    reported_at: reported_at.to_owned(),
                    findings: findings.clone(),
                });
            }
            None => applied.without_reason += 1,
        }
    }
    applied
}

/// The distinct reasons a service was filtered for, in [`Reason::ALL`] order.
pub fn reasons(s: &Service) -> Vec<Reason> {
    let Some(f) = &s.import_filter else {
        return Vec::new();
    };
    Reason::ALL
        .into_iter()
        .filter(|r| f.findings.iter().any(|x| x.reason == r.key()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "Buildfile: /mda/inc/inc-meta/build.xml

inc.edugain.report:
     [echo] Looking for errors in eduGAIN entities from /tmp/edugain-metadata.xml...
     [java] ERROR - Item https://sp.example.org/shibboleth (https://fed.example) was marked with the following Error status messages
     [java] ERROR -     check_saml1: no POST support on SAML 1.1 SP
     [java] ERROR -     check_sirtfi: SIRTFI requires a REFEDS security contact
     [java] ERROR - Item urn:example:sp (XX) was marked with the following Error status messages
     [java] ERROR -     check_entityid_prefix: entity ID urn:example:sp does not start with acceptable prefix
     [java] ERROR - Item https://idp.example.org/idp (https://fed.example) was marked with the following Error status messages
     [java] ERROR -     checkScopes/whiteSpace: scope 'login.example.org
     [java]             ' includes white space
     [java] WARN - Item https://big-logo.example.org/sp (XX) was marked with the following Warning status messages
     [java] WARN -     check_logo: mdui:Logo is longer than 50000 characters
     [java] ERROR - Item https://odd.example.org/sp (XX) was marked with the following Error status messages
     [java] ERROR -     check_something_new: a check this code does not know
     [echo] Report complete.
";

    #[test]
    fn parses_items_checks_and_continuations() {
        let r = parse_report(LOG);
        assert_eq!(r.len(), 4, "the Warning item is not a removal");
        let sp = &r["https://sp.example.org/shibboleth"];
        assert_eq!(
            sp.iter()
                .map(|f| (f.reason.as_str(), f.check.as_str()))
                .collect::<Vec<_>>(),
            [("saml1", "check_saml1"), ("sirtfi", "check_sirtfi")]
        );
        assert_eq!(sp[0].message, "no POST support on SAML 1.1 SP");
        assert_eq!(r["urn:example:sp"][0].reason, "entity-id");
        let idp = &r["https://idp.example.org/idp"][0];
        assert_eq!(
            (idp.reason.as_str(), idp.check.as_str()),
            ("scope", "checkScopes/whiteSpace")
        );
        assert_eq!(
            idp.message,
            "scope 'login.example.org ' includes white space"
        );
        let odd = &r["https://odd.example.org/sp"][0];
        assert_eq!(
            (odd.reason.as_str(), odd.check.as_str()),
            ("other", "check_something_new")
        );
    }

    #[test]
    fn maps_every_check_seen_in_published_logs() {
        for (check, reason) in [
            ("check_saml1", Reason::Saml1),
            ("check_sp_non_saml2", Reason::Saml2),
            ("check_idp_non_saml2", Reason::Saml2),
            ("check_sp_tls", Reason::Https),
            ("check_idp_tls", Reason::Https),
            ("check_reqattr", Reason::RequestedAttributes),
            ("check_bindings", Reason::Bindings),
            ("check_entityid_prefix", Reason::EntityId),
            ("check_sirtfi", Reason::Sirtfi),
            ("check_rands_member", Reason::ResearchAndScholarship),
            ("check_rands_support", Reason::ResearchAndScholarship),
            ("check_algsupport", Reason::AlgorithmSupport),
            ("check_mdui_xslt", Reason::Mdui),
            ("check_mdui_urls/url", Reason::Mdui),
            ("check_mdrpi_xslt", Reason::RegistrationInfo),
            ("check_mdattr", Reason::EntityAttributes),
            ("saml2meta/url", Reason::SamlMetadata),
            ("check_saml2meta", Reason::SamlMetadata),
            ("certificateValidation", Reason::Certificate),
            ("checkScopes/literalTail", Reason::Scope),
            ("checkScopes/domainName/noPublicSuffix", Reason::Scope),
            ("check_misc", Reason::Other),
        ] {
            assert_eq!(Reason::from_check(check), reason, "{check}");
        }
        for r in Reason::ALL {
            assert_eq!(Reason::from_key(r.key()), Some(r));
        }
    }

    #[test]
    fn finds_the_newest_log_in_the_index() {
        let html = r#"<a href="md_filter_report_1464916509.log">md_filter_report_1464916509.log</a>
            <a href="md_filter_report_1793558769.log">md_filter_report_1793558769.log</a>
            <a href="md_filter_report_1793555436.log">x</a> md_filter_report_.log md_filter_report_9.txt"#;
        assert_eq!(
            latest_report(html),
            Some(("md_filter_report_1793558769.log".to_owned(), 1793558769))
        );
        assert_eq!(latest_report("<html>no logs</html>"), None);
    }

    #[test]
    fn attaches_findings_only_to_edugain_only_services() {
        let svc = |id: &str, a: Availability| Service {
            entity_id: id.into(),
            availability: Some(a),
            ..Default::default()
        };
        let mut services = vec![
            svc(
                "https://sp.example.org/shibboleth",
                Availability::EdugainOnly,
            ),
            svc("https://late.example.org/sp", Availability::EdugainOnly),
            svc("urn:example:sp", Availability::EdugainImportedByIncommon),
        ];
        let report = parse_report(LOG);
        let a = apply(
            &mut services,
            &report,
            "https://r.example/x.log",
            "2026-11-01T18:46:09Z",
        );
        assert_eq!((a.with_reason, a.without_reason), (1, 1));
        let f = services[0].import_filter.as_ref().unwrap();
        assert_eq!(f.reported_at, "2026-11-01T18:46:09Z");
        assert_eq!(reasons(&services[0]), [Reason::Saml1, Reason::Sirtfi]);
        assert!(services[1].import_filter.is_none());
        assert!(
            services[2].import_filter.is_none(),
            "it is in InCommon's import"
        );
    }
}
