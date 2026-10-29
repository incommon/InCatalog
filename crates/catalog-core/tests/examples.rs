// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Every example the site offers (`catalog_core::examples`) is read by the rules as
//! the filters it is meant to show, and finds services on a small catalog built to
//! hold one of each, without `/v1/ask` loosening anything.

use catalog_core::examples::EXAMPLES;
use catalog_core::model::{
    Availability, Enrichment, Federation, RequestedAttribute, Service, Snapshot, Source,
};
use catalog_core::search::Catalog;

const MAIL: &str = "urn:oid:0.9.2342.19200300.100.1.3";
const ORCID: &str = "urn:oid:1.3.6.1.4.1.5923.1.1.1.16";

fn fed(code: &str, ra: &str, cc: &str, country: &str) -> Federation {
    Federation {
        code: code.into(),
        name: if code == "UK-FEDERATION" {
            "UK federation".into()
        } else {
            code.into()
        },
        registration_authority: ra.into(),
        country_codes: vec![cc.into()],
        countries: vec![country.into()],
        ..Default::default()
    }
}

struct Svc {
    id: &'static str,
    name: &'static str,
    org: &'static str,
    fed: (&'static str, &'static str),
    availability: Availability,
    categories: &'static [&'static str],
    attributes: &'static [(&'static str, &'static str)],
    environment: &'static str,
}

fn service(s: &Svc) -> Service {
    let mut out = Service {
        entity_id: s.id.into(),
        id: catalog_core::entity_hash(s.id),
        federation: Some(s.fed.0.into()),
        availability: Some(s.availability),
        categories: s.categories.iter().map(|c| c.to_string()).collect(),
        requested_attributes: s
            .attributes
            .iter()
            .map(|(name, friendly)| RequestedAttribute {
                name: (*name).into(),
                friendly_name: Some((*friendly).into()),
                ..Default::default()
            })
            .collect(),
        sources: match s.availability {
            Availability::IncommonOnly | Availability::IncommonAggregateOnly => {
                vec![Source::Incommon]
            }
            Availability::EdugainOnly => vec![Source::Edugain],
            _ => vec![Source::Incommon, Source::Edugain],
        },
        enrichment: Some(Enrichment {
            environment: s.environment.into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    out.registration.authority = Some(s.fed.1.into());
    out.organization
        .display_names
        .insert("en".into(), s.org.into());
    out.display_names.insert("en".into(), s.name.into());
    out
}

/// One service for each example to find.
fn catalog() -> Catalog {
    let swamid = ("SWAMID", "http://www.swamid.se/");
    let dfn = ("DFN-AAI", "https://www.aai.dfn.de");
    let uk = ("UK-FEDERATION", "http://ukfederation.org.uk");
    let incommon = ("INCOMMON", "https://incommon.org");
    let services = [
        Svc {
            id: "https://wiki.example.se/sp",
            name: "Research Wiki",
            org: "Example Organization",
            fed: swamid,
            availability: Availability::EdugainImportedByIncommon,
            categories: &["research-and-scholarship"],
            attributes: &[(MAIL, "mail")],
            environment: "production",
        },
        Svc {
            id: "https://data.example.de/sp",
            name: "Data Repository",
            org: "Example Organization",
            fed: dfn,
            availability: Availability::EdugainImportedByIncommon,
            categories: &["research-and-scholarship"],
            attributes: &[],
            environment: "production",
        },
        Svc {
            id: "https://orcid-tools.example.ac.uk/sp",
            name: "ORCID Linker",
            org: "Example Organization",
            fed: uk,
            availability: Availability::EdugainImportedByIncommon,
            categories: &[],
            attributes: &[(ORCID, "eduPersonOrcid")],
            environment: "production",
        },
        Svc {
            id: "https://journals.example.ac.uk/sp",
            name: "Journal of Examples",
            org: "Example Organization",
            fed: uk,
            availability: Availability::EdugainOnly,
            categories: &[],
            attributes: &[],
            environment: "production",
        },
        Svc {
            id: "https://anon.example.edu/sp",
            name: "Anonymous Survey",
            org: "Example Organization",
            fed: incommon,
            availability: Availability::IncommonAndEdugain,
            categories: &["anonymous"],
            attributes: &[],
            environment: "production",
        },
        Svc {
            id: "https://tools.example.de/sp",
            name: "Lab Scheduling",
            org: "Example Software GmbH",
            fed: dfn,
            availability: Availability::EdugainImportedByIncommon,
            categories: &[],
            attributes: &[],
            environment: "production",
        },
        Svc {
            id: "https://canvas-test.example.edu/sp",
            name: "Canvas (Test)",
            org: "Example Organization",
            fed: incommon,
            availability: Availability::IncommonOnly,
            categories: &[],
            attributes: &[],
            environment: "production",
        },
    ];
    Catalog::new(Snapshot {
        services: services.iter().map(service).collect(),
        federations: vec![
            fed(swamid.0, swamid.1, "SE", "Sweden"),
            fed(dfn.0, dfn.1, "DE", "Germany"),
            fed(uk.0, uk.1, "GB", "United Kingdom"),
            fed(incommon.0, incommon.1, "US", "United States"),
        ],
        ..Default::default()
    })
}

#[test]
fn every_example_reads_as_intended_and_finds_services() {
    let c = catalog();
    let mut failures = Vec::new();
    for ex in EXAMPLES {
        let reading = c.interpret(ex.query);
        let params = serde_json::to_value(&reading.params).unwrap();
        for (field, want) in ex.reads_as {
            if params[*field] != *want {
                failures.push(format!(
                    "{:?}: {field} is {}, not {want:?}",
                    ex.query, params[*field]
                ));
            }
        }
        if !ex.reads_as.iter().any(|(f, _)| *f == "q") && !params["q"].is_null() {
            failures.push(format!("{:?}: left words {}", ex.query, params["q"]));
        }
        let set = params
            .as_object()
            .unwrap()
            .iter()
            .filter(|(k, v)| !v.is_null() && !ex.reads_as.iter().any(|(f, _)| f == k))
            .map(|(k, _)| k.clone())
            .collect::<Vec<_>>();
        if !set.is_empty() {
            failures.push(format!("{:?}: also set {set:?}", ex.query));
        }
        let answer = c.ask(reading);
        if answer.total == 0 || answer.relaxed.is_some() {
            failures.push(format!(
                "{:?}: found {} (loosened: {:?})",
                ex.query, answer.total, answer.relaxed
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_behavior_has_an_example() {
    let fields: Vec<&str> = EXAMPLES
        .iter()
        .flat_map(|e| e.reads_as.iter().map(|(f, _)| *f))
        .collect();
    for f in [
        "federation",
        "country",
        "region",
        "category",
        "availability",
        "attribute",
        "environment",
        "sharing",
        "sector",
    ] {
        assert!(fields.contains(&f), "no example shows {f}");
    }
    assert!(
        EXAMPLES.iter().any(|e| e
            .reads_as
            .iter()
            .any(|(f, v)| *f == "federation" && v.contains(','))),
        "no example names several federations"
    );
}
