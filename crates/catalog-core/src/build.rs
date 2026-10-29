// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Merge the InCommon and eduGAIN aggregates into one set of services.
//!
//! The same entity commonly appears in both files. They are joined on entityID and one
//! copy is kept: InCommon's when InCommon registered it, otherwise eduGAIN's. Export
//! pipelines can rewrite metadata (InCommon does not export its deprecated Research &
//! Scholarship URI), so the registrar's nearest feed is the faithful one.
//!
//! An entityID can also be registered by two federations at once — a "clash". eduGAIN
//! publishes only one registration, so a vendor registered by both InCommon and the UK
//! federation appears in eduGAIN's file as a UK entity and in InCommon's as an InCommon
//! one. Each copy is kept until [`Builder::finish`] so the clash can be reported rather
//! than silently resolved in favor of whichever file was read last.

use std::collections::{BTreeMap, HashMap};
use std::io::BufRead;

use anyhow::Result;

use crate::categories::{self, DEPRECATED_INCOMMON_RS};
use crate::curation::CurationFile;
use crate::extract::{extract, Extracted};
use crate::federations::FederationIndex;
use crate::model::{Availability, Federation, Service, Source, SourceInfo};
use crate::xml::for_each_entity;
use crate::INCOMMON_REGISTRAR;

#[derive(Default)]
struct Copies {
    incommon: Option<Service>,
    edugain: Option<Service>,
}

#[derive(Default)]
pub struct Builder {
    services: HashMap<String, Copies>,
    idps: HashMap<String, Option<String>>,
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stream one aggregate into the builder. Fills in the entity counts on `info`.
    pub fn add_aggregate<R: BufRead>(
        &mut self,
        source: Source,
        reader: R,
        info: &mut SourceInfo,
    ) -> Result<()> {
        let (mut sps, mut idps) = (0, 0);
        let agg = for_each_entity(reader, |el| match extract(&el) {
            Extracted::Sp(s) => {
                sps += 1;
                self.add_service(source, *s);
            }
            Extracted::Idp {
                entity_id,
                registrar,
            } => {
                idps += 1;
                self.idps.entry(entity_id).or_insert(registrar);
            }
            Extracted::Other => {}
        })?;
        info.source = Some(source);
        info.name = agg.name;
        info.valid_until = agg.valid_until;
        info.entities = agg.entity_count;
        info.service_providers = sps;
        info.identity_providers = idps;
        Ok(())
    }

    pub fn add_service(&mut self, source: Source, s: Service) {
        let copies = self.services.entry(s.entity_id.clone()).or_default();
        let slot = match source {
            Source::Incommon => &mut copies.incommon,
            Source::Edugain => &mut copies.edugain,
        };
        slot.get_or_insert(s);
    }

    pub fn finish(
        self,
        federations: &[Federation],
        curation: Option<&CurationFile>,
    ) -> (Vec<Service>, BTreeMap<String, usize>) {
        let feds = FederationIndex::new(federations);
        let mut out: Vec<Service> = self
            .services
            .into_values()
            .map(|c| {
                let mut s = canonical(c);
                s.availability = Some(availability(&s));
                s.federation = s
                    .registrar()
                    .and_then(|r| feds.get(r))
                    .map(|f| f.code.clone());
                s.notes = notes(&s);
                if let Some(c) = curation {
                    c.apply(&mut s);
                }
                s
            })
            .collect();
        out.sort_by(|a, b| a.entity_id.cmp(&b.entity_id));

        let mut idp_counts = BTreeMap::new();
        for registrar in self.idps.into_values() {
            *idp_counts.entry(registrar.unwrap_or_default()).or_insert(0) += 1;
        }
        (out, idp_counts)
    }
}

fn is_incommon(s: &Service) -> bool {
    s.registrar() == Some(INCOMMON_REGISTRAR)
}

fn canonical(c: Copies) -> Service {
    let sources: Vec<Source> = [
        c.incommon.as_ref().map(|_| Source::Incommon),
        c.edugain.as_ref().map(|_| Source::Edugain),
    ]
    .into_iter()
    .flatten()
    .collect();
    let (mut keep, other) = match (c.incommon, c.edugain) {
        (Some(ic), eg) if is_incommon(&ic) => (ic, eg),
        (ic, Some(eg)) => (eg, ic),
        (Some(ic), None) => (ic, None),
        (None, None) => unreachable!("a Copies entry always holds at least one copy"),
    };
    if let Some(other) = other {
        if other.registrar() != keep.registrar() {
            if let Some(r) = other.registration.authority {
                keep.also_registered_by.push(r);
            }
        }
    }
    keep.sources = sources;
    keep
}

fn availability(s: &Service) -> Availability {
    let in_ic = s.sources.contains(&Source::Incommon);
    let in_eg = s.sources.contains(&Source::Edugain);
    match (is_incommon(s), in_ic, in_eg) {
        (true, _, true) => Availability::IncommonAndEdugain,
        (true, _, false) => Availability::IncommonOnly,
        (false, true, true) => Availability::EdugainImportedByIncommon,
        (false, false, _) => Availability::EdugainOnly,
        (false, true, false) => Availability::IncommonAggregateOnly,
    }
}

/// Observations about the metadata as published. These describe configuration,
/// not the operator.
fn notes(s: &Service) -> Vec<String> {
    let mut n = Vec::new();
    let has = |uris: &[&str]| {
        s.entity_categories
            .iter()
            .any(|c| uris.contains(&c.as_str()))
    };
    let rs = categories::get("research-and-scholarship")
        .map(|d| d.uris)
        .unwrap_or(&[]);
    let refeds_rs: Vec<&str> = rs
        .iter()
        .copied()
        .filter(|u| !DEPRECATED_INCOMMON_RS.contains(u))
        .collect();
    if has(DEPRECATED_INCOMMON_RS) && !has(&refeds_rs) {
        n.push(
            "Research & Scholarship is asserted only under the deprecated InCommon URI, which is not exported to eduGAIN as R&S."
                .to_owned(),
        );
    }
    if has(categories::AUTHN_PROFILES) {
        n.push(
            "A REFEDS authentication profile (MFA/SFA) is filed as an entity category; these profiles are signaled per authentication, not in metadata."
                .to_owned(),
        );
    }
    if !s.entity_category_support.is_empty() {
        n.push(
            "Carries entity-category-support values, which describe identity-provider release behavior and are unusual on a service provider."
                .to_owned(),
        );
    }
    let sirtfi = s.categories.iter().any(|c| c == "sirtfi" || c == "sirtfi2");
    if sirtfi && !s.contacts.iter().any(|c| c.kind == "security") {
        n.push("Asserts Sirtfi but publishes no REFEDS security contact.".to_owned());
    }
    let coco = s
        .categories
        .iter()
        .any(|c| c.starts_with("code-of-conduct"));
    if coco && s.privacy_statement_urls.is_empty() {
        n.push("Asserts a Code of Conduct but publishes no privacy statement URL.".to_owned());
    }
    for r in &s.also_registered_by {
        n.push(format!(
            "This entityID is also registered by {r}; the two federations publish separate registrations (an entityID clash)."
        ));
    }
    if s.display_names.is_empty() {
        n.push(
            "No MDUI DisplayName; the catalog name falls back to the service or organization name."
                .to_owned(),
        );
    }
    n
}
