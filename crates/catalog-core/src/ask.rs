// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Turn a reading of a natural-language query into results, loosening it step by
//! step when the literal reading finds nothing.
//!
//! A person who types "journals not in InCommon" should not get an empty page
//! because no journal happens to be eduGAIN-only. They should get the closest
//! results and be told what was loosened. The steps, stopping at the first that
//! finds anything:
//!
//! 1. every filter, every word
//! 2. every filter, most words (half), then any word
//! 3. drop one filter at a time, softest first
//! 4. the words alone, then the filters alone
//!
//! Each step after the first two produces a sentence saying what changed.

use serde::Serialize;
use utoipa::ToSchema;

use crate::nl::{Interpretation, InterpretedParams};
use crate::search::{Catalog, ServiceQuery};

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AskResult {
    pub reading: Interpretation,
    /// The filters that produced results; pass these to `/v1/services`.
    pub params: InterpretedParams,
    /// `all`, `any`, or `some` (see `/v1/services`).
    #[serde(rename = "match")]
    pub match_mode: String,
    pub total: usize,
    /// What was loosened to find results, when the full reading found none.
    pub relaxed: Option<String>,
}

/// Fields in the order they are given up: the ones a reader is most likely to
/// have over-read go first, the ones a person states most deliberately go last.
const SOFTEST_FIRST: &[(&str, &str)] = &[
    ("attribute", "Requests"),
    ("country", "Country"),
    ("region", "Region"),
    ("environment", "Environment"),
    ("source", "Seen in"),
    ("federation", "Registered by"),
    ("provider_type", "Provider type"),
    ("availability", "Availability"),
    ("category", "Category"),
];

impl Catalog {
    pub fn ask(&self, reading: Interpretation) -> AskResult {
        let base = reading.params.clone();
        let words = base
            .q
            .as_deref()
            .map(|q| q.split_whitespace().count())
            .unwrap_or(0);
        let modes: &[&str] = match words {
            0 | 1 => &["all"],
            _ => &["all", "any", "some"],
        };
        let loosest = *modes.last().unwrap();

        let mut steps: Vec<(InterpretedParams, &str, Option<String>)> = Vec::new();
        for m in modes {
            let note = (*m == "some").then(|| {
                format!("No service mentions all of “{}”; showing services that mention some of those words.", base.q.as_deref().unwrap_or(""))
            });
            steps.push((base.clone(), m, note));
        }
        let set: Vec<(&str, &str)> = SOFTEST_FIRST
            .iter()
            .copied()
            .filter(|(f, _)| get(&base, f).is_some())
            .collect();
        for (field, label) in &set {
            let mut p = base.clone();
            let value = take(&mut p, field).unwrap_or_default();
            let shown = describe(self, field, &value);
            steps.push((
                p,
                loosest,
                Some(format!(
                    "Nothing matched with {label}: {shown}, so that filter was dropped."
                )),
            ));
        }
        if base.q.is_some() && set.len() > 1 {
            let words_only = InterpretedParams {
                q: base.q.clone(),
                ..Default::default()
            };
            steps.push((
                words_only,
                loosest,
                Some(format!("Nothing matched “{}” with those filters together; showing services that match the words alone.", base.q.as_deref().unwrap_or(""))),
            ));
        }
        if base.q.is_some() && !set.is_empty() {
            let filters_only = InterpretedParams {
                q: None,
                ..base.clone()
            };
            steps.push((
                filters_only,
                "all",
                Some(format!(
                    "No service matching those filters mentions “{}”; showing all of them instead.",
                    base.q.as_deref().unwrap_or("")
                )),
            ));
        }

        for (params, mode, note) in steps {
            let total = self.count(&params, mode);
            if total > 0 {
                return AskResult {
                    reading,
                    params,
                    match_mode: mode.to_owned(),
                    total,
                    relaxed: note,
                };
            }
        }
        let relaxed = base
            .q
            .as_ref()
            .map(|q| format!("Nothing in federation metadata matches “{q}”."));
        AskResult {
            reading,
            params: base,
            match_mode: modes[0].to_owned(),
            total: 0,
            relaxed,
        }
    }

    fn count(&self, p: &InterpretedParams, mode: &str) -> usize {
        self.search(&ServiceQuery {
            q: p.q.clone(),
            federation: p.federation.clone(),
            country: p.country.clone(),
            region: p.region.clone(),
            environment: p.environment.clone(),
            category: p.category.clone(),
            availability: p.availability.clone(),
            attribute: p.attribute.clone(),
            source: p.source.clone(),
            provider_type: p.provider_type.clone(),
            match_mode: Some(mode.to_owned()),
            per_page: Some(1),
            facets: Some(false),
            ..Default::default()
        })
        .total
    }
}

fn get<'a>(p: &'a InterpretedParams, field: &str) -> Option<&'a String> {
    match field {
        "federation" => p.federation.as_ref(),
        "country" => p.country.as_ref(),
        "region" => p.region.as_ref(),
        "environment" => p.environment.as_ref(),
        "category" => p.category.as_ref(),
        "availability" => p.availability.as_ref(),
        "attribute" => p.attribute.as_ref(),
        "source" => p.source.as_ref(),
        "provider_type" => p.provider_type.as_ref(),
        _ => None,
    }
}

fn take(p: &mut InterpretedParams, field: &str) -> Option<String> {
    match field {
        "federation" => p.federation.take(),
        "country" => p.country.take(),
        "region" => p.region.take(),
        "environment" => p.environment.take(),
        "category" => p.category.take(),
        "availability" => p.availability.take(),
        "attribute" => p.attribute.take(),
        "source" => p.source.take(),
        "provider_type" => p.provider_type.take(),
        _ => None,
    }
}

/// Categories must all hold ("R&S + Sirtfi"); the rest are alternatives
/// ("SWAMID or DFN-AAI", "Sweden or Norway", "Test or Staging").
fn describe(c: &Catalog, field: &str, value: &str) -> String {
    value
        .split(',')
        .map(|v| c.vocab().label(field, v))
        .collect::<Vec<_>>()
        .join(if field == "category" { " + " } else { " or " })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Availability, Federation, Service, Snapshot};

    fn catalog() -> Catalog {
        let svc = |id: &str, name: &str, avail: Availability| {
            let mut s = Service {
                entity_id: id.into(),
                id: crate::entity_hash(id),
                availability: Some(avail),
                ..Default::default()
            };
            s.display_names.insert("en".into(), name.into());
            s
        };
        Catalog::new(Snapshot {
            services: vec![
                svc(
                    "https://a/",
                    "Journal of Examples",
                    Availability::IncommonAndEdugain,
                ),
                svc("https://b/", "Lab Notebook", Availability::EdugainOnly),
            ],
            federations: vec![Federation {
                code: "X".into(),
                name: "X".into(),
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    fn reading(c: &Catalog, q: &str) -> Interpretation {
        c.interpret(q)
    }

    #[test]
    fn exact_reading_is_not_relaxed() {
        let c = catalog();
        let r = c.ask(reading(&c, "journal of examples"));
        assert_eq!(r.total, 1);
        assert_eq!(r.relaxed, None);
    }

    #[test]
    fn drops_the_filter_that_empties_results_and_says_so() {
        let c = catalog();
        let r = c.ask(reading(&c, "journals not in InCommon"));
        assert_eq!(
            r.reading.params.availability.as_deref(),
            Some("edugain-only")
        );
        assert_eq!(r.params.availability, None);
        assert_eq!(r.total, 1);
        assert!(r.relaxed.unwrap().contains("Availability"));
    }

    #[test]
    fn falls_back_to_some_of_the_words() {
        let c = catalog();
        let r = c.ask(reading(&c, "notebook for chemistry teaching"));
        assert_eq!(r.match_mode, "some");
        assert_eq!(r.total, 1);
        assert!(r.relaxed.is_some());
    }

    #[test]
    fn several_federations_are_described_as_alternatives() {
        let c = catalog();
        let mut r = reading(&c, "journal");
        r.params.federation = Some("X,Y".into());
        let a = c.ask(r);
        assert_eq!(a.total, 1);
        assert!(a.relaxed.unwrap().contains("Registered by: X or Y"));
    }

    #[test]
    fn says_when_nothing_matches_at_all() {
        let c = catalog();
        let r = c.ask(reading(&c, "adobe"));
        assert_eq!(r.total, 0);
        assert!(r.relaxed.unwrap().contains("adobe"));
    }

    #[test]
    fn drops_an_environment_that_finds_nothing() {
        let c = catalog();
        let r = c.ask(reading(&c, "test instances of the lab notebook"));
        assert_eq!(r.reading.params.environment.as_deref(), Some("test"));
        assert_eq!(r.params.environment, None);
        assert_eq!(r.total, 1);
        assert!(r.relaxed.unwrap().contains("Environment: Test"));

        let mut r = reading(&c, "journal");
        r.params.environment = Some("test,staging".into());
        assert!(c
            .ask(r)
            .relaxed
            .unwrap()
            .contains("Environment: Test or Staging"));
    }

    #[test]
    fn drops_a_region_that_finds_nothing() {
        let c = catalog();
        let mut r = reading(&c, "journal");
        r.params.region = Some("brics,eu".into());
        let a = c.ask(r);
        assert_eq!(a.params.region, None);
        assert_eq!(a.total, 1);
        assert!(a
            .relaxed
            .unwrap()
            .contains("Region: BRICS or the European Union"));
    }
}
