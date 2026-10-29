// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! The sharing rules against the hand-labeled sample in
//! `fixtures/sharing-sample.json`: per-campus Zoom, Canvas, and other tenants,
//! campuses' own services, publishers, the Internet2 IAM platform, research
//! collaborations, and federation operators' services. The rules decide only
//! what they are near certain of and leave the rest to Claude, so this checks
//! their precision, and reports how much of the sample they decide.
//! `catalog-ingest` runs the same sample through the prompt (`sharing::eval`).

use catalog_core::model::Service;
use catalog_core::sharing::{classify, Cluster, Sharing};
use serde_json::Value;

pub struct Sample {
    pub label: Sharing,
    pub why: String,
    pub cluster: Option<Cluster>,
    pub service: Service,
}

fn samples() -> Vec<Sample> {
    let raw: Value = serde_json::from_str(include_str!("fixtures/sharing-sample.json")).unwrap();
    raw["samples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            // The fixture keeps only the fields that bear on sharing.
            let mut service = serde_json::to_value(Service::default()).unwrap();
            for (k, v) in s["service"].as_object().unwrap() {
                service[k] = v.clone();
            }
            Sample {
                label: Sharing::from_key(s["label"].as_str().unwrap()).unwrap(),
                why: s["why"].as_str().unwrap().to_owned(),
                cluster: serde_json::from_value(s["cluster"].clone()).unwrap(),
                service: serde_json::from_value(service).unwrap(),
            }
        })
        .collect()
}

#[test]
fn the_rules_are_precise_on_the_sample() {
    let samples = samples();
    assert_eq!(samples.len(), 60);
    let mut wrong = Vec::new();
    let mut decided = [0usize; 2];
    let mut right = [0usize; 2];
    for s in &samples {
        let r = classify(&s.service, s.cluster.as_ref());
        let i = match r.value {
            Sharing::Bilateral => 0,
            Sharing::Multilateral => 1,
            Sharing::Unknown => continue,
        };
        decided[i] += 1;
        if r.value == s.label {
            right[i] += 1;
        } else {
            wrong.push(format!(
                "{} ({}): {:?}",
                s.service.entity_id, s.why, r.reason
            ));
        }
    }
    for (i, label) in ["bilateral", "multilateral"].iter().enumerate() {
        let of = samples.iter().filter(|s| s.label.key() == *label).count();
        println!(
            "{label}: decided {} of {of} labeled, {} right (precision {:.2})",
            decided[i],
            right[i],
            right[i] as f64 / decided[i].max(1) as f64
        );
    }
    assert!(
        wrong.is_empty(),
        "the rules got these wrong:\n{}",
        wrong.join("\n")
    );
    // Enough coverage that the rules earn their place; Claude reads the rest.
    assert!(decided[0] >= 20 && decided[1] >= 10, "{decided:?}");
}
