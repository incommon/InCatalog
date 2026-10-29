// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Reduce one `EntityDescriptor` to the catalog's view of it.

use crate::categories;
use crate::model::{
    Contact, EntityAttribute, LangMap, Logo, Organization, Registration, RequestedAttribute,
    Service,
};
use crate::ns;
use crate::xml::Element;

pub enum Extracted {
    Sp(Box<Service>),
    Idp {
        entity_id: String,
        registrar: Option<String>,
    },
    /// Attribute authorities and anything else without an SSO role.
    Other,
}

pub fn extract(entity: &Element) -> Extracted {
    let Some(entity_id) = entity
        .attr("entityID")
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Extracted::Other;
    };
    let ext = entity.child(ns::MD, "Extensions");
    let registration = ext.map(registration).unwrap_or_default();

    let Some(sp) = entity.child(ns::MD, "SPSSODescriptor") else {
        return if entity.child(ns::MD, "IDPSSODescriptor").is_some() {
            Extracted::Idp {
                entity_id: entity_id.to_owned(),
                registrar: registration.authority,
            }
        } else {
            Extracted::Other
        };
    };

    let mut s = Service {
        id: crate::entity_hash(entity_id),
        entity_id: entity_id.to_owned(),
        registration,
        ..Default::default()
    };

    if let Some(ext) = ext {
        entity_attributes(ext, &mut s);
    }
    s.categories = categories::classify(&s.entity_categories, &s.assurance_certifications);

    s.protocols = sp
        .attr("protocolSupportEnumeration")
        .map(|p| p.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default();

    if let Some(sp_ext) = sp.child(ns::MD, "Extensions") {
        if let Some(ui) = sp_ext.child(ns::MDUI, "UIInfo") {
            ui_info(ui, &mut s);
        }
        s.supports_discovery_response = sp_ext.child(ns::IDPDISC, "DiscoveryResponse").is_some();
    }

    for kd in sp.children(ns::MD, "KeyDescriptor") {
        match kd.attr("use") {
            Some("signing") => s.has_signing_key = true,
            Some("encryption") => s.has_encryption_key = true,
            // No `use` means the key serves both purposes.
            _ => {
                s.has_signing_key = true;
                s.has_encryption_key = true;
            }
        }
    }
    s.name_id_formats = dedup(
        sp.children(ns::MD, "NameIDFormat")
            .map(|e| e.text().to_owned()),
    );
    s.acs_bindings = dedup(
        sp.children(ns::MD, "AssertionConsumerService")
            .filter_map(|e| e.attr("Binding").map(str::to_owned)),
    );
    s.endpoint_hosts = dedup(
        sp.children(ns::MD, "AssertionConsumerService")
            .filter_map(|e| e.attr("Location"))
            .filter_map(url_host),
    );

    for acs in sp.children(ns::MD, "AttributeConsumingService") {
        lang_into(acs.children(ns::MD, "ServiceName"), &mut s.service_names);
        lang_into(
            acs.children(ns::MD, "ServiceDescription"),
            &mut s.service_descriptions,
        );
        for ra in acs.children(ns::MD, "RequestedAttribute") {
            let Some(name) = ra.attr("Name") else {
                continue;
            };
            if s.requested_attributes.iter().any(|r| r.name == name) {
                continue;
            }
            s.requested_attributes.push(RequestedAttribute {
                name: name.to_owned(),
                friendly_name: ra.attr("FriendlyName").map(str::to_owned),
                name_format: ra.attr("NameFormat").map(str::to_owned),
                required: matches!(ra.attr("isRequired"), Some("true") | Some("1")),
            });
        }
    }

    if let Some(org) = entity.child(ns::MD, "Organization") {
        s.organization = Organization {
            names: lang_map(org.children(ns::MD, "OrganizationName")),
            display_names: lang_map(org.children(ns::MD, "OrganizationDisplayName")),
            urls: lang_map(org.children(ns::MD, "OrganizationURL")),
        };
    }

    for cp in entity.children(ns::MD, "ContactPerson") {
        let kind = if cp.attr_ns(ns::REMD, "contactType") == Some(ns::REFEDS_SECURITY_CONTACT) {
            "security".to_owned()
        } else {
            cp.attr("contactType").unwrap_or("other").to_owned()
        };
        let emails = dedup(
            cp.children(ns::MD, "EmailAddress")
                .map(|e| e.text().trim_start_matches("mailto:").to_owned()),
        );
        if !emails.is_empty() {
            s.contacts.push(Contact { kind, emails });
        }
    }

    Extracted::Sp(Box::new(s))
}

fn registration(ext: &Element) -> Registration {
    let Some(ri) = ext.child(ns::MDRPI, "RegistrationInfo") else {
        return Registration::default();
    };
    Registration {
        authority: ri
            .attr("registrationAuthority")
            .map(|s| s.trim().to_owned()),
        instant: ri.attr("registrationInstant").map(str::to_owned),
        policies: lang_map(ri.children(ns::MDRPI, "RegistrationPolicy")),
    }
}

fn entity_attributes(ext: &Element, s: &mut Service) {
    for ea in ext.children(ns::MDATTR, "EntityAttributes") {
        for attr in ea.children(ns::SAML, "Attribute") {
            let Some(name) = attr.attr("Name") else {
                continue;
            };
            let values = dedup(
                attr.children(ns::SAML, "AttributeValue")
                    .map(|v| v.text().to_owned()),
            );
            let target = match name {
                ns::ATTR_ENTITY_CATEGORY => &mut s.entity_categories,
                ns::ATTR_ENTITY_CATEGORY_SUPPORT => &mut s.entity_category_support,
                ns::ATTR_ASSURANCE_CERTIFICATION => &mut s.assurance_certifications,
                _ => {
                    debug_assert!(!categories::is_known_attribute(name));
                    s.other_entity_attributes.push(EntityAttribute {
                        name: name.to_owned(),
                        values,
                    });
                    continue;
                }
            };
            for v in values {
                if !target.contains(&v) {
                    target.push(v);
                }
            }
        }
    }
}

fn ui_info(ui: &Element, s: &mut Service) {
    lang_into(ui.children(ns::MDUI, "DisplayName"), &mut s.display_names);
    lang_into(ui.children(ns::MDUI, "Description"), &mut s.descriptions);
    lang_into(
        ui.children(ns::MDUI, "InformationURL"),
        &mut s.information_urls,
    );
    lang_into(
        ui.children(ns::MDUI, "PrivacyStatementURL"),
        &mut s.privacy_statement_urls,
    );
    for kw in ui.children(ns::MDUI, "Keywords") {
        // MDUI keywords are space-separated; '+' stands in for a space inside one.
        for k in kw.text().split_whitespace() {
            let k = k.replace('+', " ");
            if !s.keywords.contains(&k) {
                s.keywords.push(k);
            }
        }
    }
    for logo in ui.children(ns::MDUI, "Logo") {
        let url = logo.text();
        if url.is_empty() {
            continue;
        }
        s.logos.push(Logo {
            url: url.to_owned(),
            width: logo.attr("width").and_then(|v| v.parse().ok()),
            height: logo.attr("height").and_then(|v| v.parse().ok()),
            lang: logo.lang().map(str::to_owned),
            broken: false,
        });
    }
}

fn lang_map<'a>(els: impl Iterator<Item = &'a Element>) -> LangMap {
    let mut m = LangMap::new();
    lang_into(els, &mut m);
    m
}

/// First value per language wins, matching how most consumers render MDUI.
fn lang_into<'a>(els: impl Iterator<Item = &'a Element>, m: &mut LangMap) {
    for e in els {
        let v = e.text();
        if v.is_empty() {
            continue;
        }
        m.entry(e.lang().unwrap_or("").to_owned())
            .or_insert_with(|| v.to_owned());
    }
}

/// The lowercase host of an `http(s)` URL.
fn url_host(url: &str) -> Option<String> {
    let rest = url.trim().split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?.split(':').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

fn dedup(it: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in it {
        if !v.is_empty() && !out.contains(&v) {
            out.push(v);
        }
    }
    out
}
