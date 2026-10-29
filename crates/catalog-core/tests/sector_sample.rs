// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! The sector rules against the hand-labeled organizations in
//! `fixtures/sector-sample.json`: universities in several languages, hospitals,
//! labs, agencies, libraries, networks, publishers, nonprofits, and companies, with
//! the names that trip a careless rule (university presses, library products,
//! companies named for what they sell to). The rules decide only what they are
//! sure of and leave the rest to Claude, so this checks precision per sector and
//! reports coverage. `catalog-ingest` runs the same sample through the prompt
//! (`sector::tests::eval`).

use std::collections::HashSet;

use catalog_core::sector::{rules, Sector};
use serde_json::Value;

#[test]
fn the_rules_are_precise_on_the_sample() {
    let raw: Value = serde_json::from_str(include_str!("fixtures/sector-sample.json")).unwrap();
    let samples = raw["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 60);
    let feds: HashSet<String> = ["swamid.se", "aaf.edu.au", "dfn.de"]
        .map(String::from)
        .into();
    let mut wrong = Vec::new();
    let mut decided = vec![0usize; Sector::ALL.len()];
    let mut right = vec![0usize; Sector::ALL.len()];
    let mut labeled = vec![0usize; Sector::ALL.len()];
    for s in samples {
        let want: Vec<Sector> = s["sectors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| Sector::from_key(v.as_str().unwrap()).unwrap())
            .collect();
        for w in &want {
            labeled[*w as usize] += 1;
        }
        let r = rules(s["name"].as_str().unwrap(), s["host"].as_str(), &feds);
        for (got, why) in r.sectors {
            decided[got as usize] += 1;
            if want.contains(&got) {
                right[got as usize] += 1;
            } else {
                wrong.push(format!("{} is not {} ({why})", s["name"], got.key()));
            }
        }
    }
    for (i, s) in Sector::ALL.iter().enumerate() {
        println!(
            "{:11} labeled {:2}, decided {:2}, right {:2} (precision {:.2})",
            s.key(),
            labeled[i],
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
    let total: usize = decided.iter().sum();
    assert!(
        total >= 35,
        "the rules decide too little of the sample: {total}"
    );
}
