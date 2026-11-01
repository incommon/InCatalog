// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Editorial overlay: what federation metadata cannot say.
//!
//! Metadata has no notion of "commercial vs. non-commercial provider", subject tags,
//! or a plain-language summary written for a catalog reader. This file supplies them,
//! keyed by entityID (one service) or by organization display name (every service an
//! organization registers — useful for vendors that register one SP per customer).
//!
//! Curated values are stored on `Service::curated`, never merged into the
//! metadata-derived fields, so a reader can always tell which is which.
//!
//! It is also the only place a possible match against OFAC's sanctions lists is
//! confirmed or dismissed (`ofac`, see [`crate::ofac::Decision`]).

use std::collections::HashMap;

use serde::Deserialize;

use crate::model::{pick, Curated, Service};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CurationFile {
    #[serde(default)]
    pub services: HashMap<String, CuratedEntry>,
    #[serde(default)]
    pub providers: HashMap<String, CuratedEntry>,
    /// Decisions on possible OFAC matches.
    #[serde(default)]
    pub ofac: Vec<crate::ofac::Decision>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CuratedEntry {
    pub provider_type: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub summary: Option<String>,
    #[serde(default)]
    pub hidden: bool,
}

impl CurationFile {
    pub fn from_json(bytes: &[u8]) -> anyhow::Result<Self> {
        Ok(serde_json::from_slice(bytes)?)
    }

    /// Provider-level entry first, then the service-level entry on top.
    pub fn apply(&self, s: &mut Service) {
        let provider = provider_name(s).and_then(|n| {
            self.providers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(n))
                .map(|(_, v)| v)
        });
        let service = self.services.get(&s.entity_id);
        if provider.is_none() && service.is_none() {
            return;
        }
        let mut c = Curated::default();
        for e in provider.into_iter().chain(service) {
            if e.provider_type.is_some() {
                c.provider_type = e.provider_type.clone();
            }
            if e.summary.is_some() {
                c.summary = e.summary.clone();
            }
            for t in &e.tags {
                if !c.tags.contains(t) {
                    c.tags.push(t.clone());
                }
            }
            c.hidden |= e.hidden;
        }
        s.curated = Some(c);
    }
}

/// The name a service's provider is grouped under: organization display name, then
/// organization name. Free text chosen by the registrant, not a registry identity.
pub fn provider_name(s: &Service) -> Option<&str> {
    pick(&s.organization.display_names, None).or_else(|| pick(&s.organization.names, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed overlays parse, and each OFAC decision names one entry and one
    /// provider or service: a misspelled key would otherwise be dropped silently.
    #[test]
    fn the_committed_overlays_are_well_formed() {
        for (name, bytes) in [
            (
                "curation.json",
                &include_bytes!("../../../curation/curation.json")[..],
            ),
            (
                "curation.example.json",
                &include_bytes!("../../../curation/curation.example.json")[..],
            ),
        ] {
            let file = CurationFile::from_json(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            for d in &file.ofac {
                let (list, uid) = d.entry.split_once(':').unwrap_or_default();
                assert!(
                    matches!(list, "sdn" | "consolidated") && uid.parse::<u64>().is_ok(),
                    "{name}: entry {}",
                    d.entry
                );
                assert!(
                    d.provider.is_some() != d.service.is_some(),
                    "{name}: {} names neither or both of provider and service",
                    d.entry
                );
                assert!(
                    chrono::NaiveDate::parse_from_str(&d.date, "%Y-%m-%d").is_ok(),
                    "{name}: {} date {}",
                    d.entry,
                    d.date
                );
            }
        }
    }
}
