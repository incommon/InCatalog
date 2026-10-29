// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

use std::io::BufReader;

use catalog_core::build::Builder;
use catalog_core::curation::CurationFile;
use catalog_core::federations::parse_edugain_federations;
use catalog_core::model::{Availability, Snapshot, Source, SourceInfo};
use catalog_core::search::{Catalog, ServiceQuery};

fn fixture(name: &str) -> BufReader<std::fs::File> {
    let p = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    BufReader::new(std::fs::File::open(p).unwrap())
}

fn catalog(curation: Option<&CurationFile>) -> Catalog {
    let mut b = Builder::new();
    let mut ic = SourceInfo::default();
    let mut eg = SourceInfo::default();
    b.add_aggregate(Source::Incommon, fixture("incommon.xml"), &mut ic)
        .unwrap();
    b.add_aggregate(Source::Edugain, fixture("edugain.xml"), &mut eg)
        .unwrap();
    assert_eq!(
        (ic.entities, ic.service_providers, ic.identity_providers),
        (4, 3, 1)
    );
    assert_eq!(ic.valid_until.as_deref(), Some("2099-01-01T00:00:00Z"));
    let feds = parse_edugain_federations(
        &std::fs::read(format!(
            "{}/tests/fixtures/feds.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap();
    let (services, idp_counts) = b.finish(&feds, curation);
    Catalog::new(Snapshot {
        schema_version: 1,
        sources: vec![ic, eg],
        federations: feds,
        services,
        idp_counts,
        ..Default::default()
    })
}

#[test]
fn parses_and_merges() {
    let c = catalog(None);
    let wiki = c
        .service_by_entity_id("https://sp.example.edu/shibboleth")
        .unwrap();
    let s = &wiki.service;
    // InCommon's own copy wins for an InCommon registration.
    assert_eq!(wiki.name, "Example Research Wiki");
    assert_eq!(s.availability, Some(Availability::IncommonAndEdugain));
    assert_eq!(s.sources, vec![Source::Incommon, Source::Edugain]);
    assert_eq!(
        s.display_names.get("de").map(String::as_str),
        Some("Beispiel-Wiki")
    );
    assert_eq!(s.descriptions["en"], "A wiki for research & collaboration.");
    assert!(s.keywords.contains(&"open science".to_owned()));
    // The REFEDS security contact lives in a second namespace.
    assert!(s
        .contacts
        .iter()
        .any(|c| c.kind == "security" && c.emails == ["security@example.edu"]));
    assert!(s
        .categories
        .contains(&"research-and-scholarship".to_owned()));
    assert!(s.categories.contains(&"sirtfi".to_owned()));
    assert!(s
        .notes
        .iter()
        .any(|n| n.contains("deprecated InCommon URI")));
    assert_eq!(s.requested_attributes.len(), 2);
    assert!(s.requested_attributes[0].required);
    assert!(s.has_signing_key && s.has_encryption_key && s.supports_discovery_response);
    // Endpoint hosts are lowercase, without ports or repeats.
    assert_eq!(s.endpoint_hosts, ["sp.example.edu"]);
    assert_eq!(
        wiki.environment.value,
        catalog_core::environment::Environment::Unknown
    );
    assert_eq!(
        wiki.federation.as_ref().map(|f| f.code.as_str()),
        Some("INCOMMON")
    );
    assert!(wiki.links.mdq.as_deref().unwrap().ends_with(&s.id));

    let only = c
        .service_by_entity_id("https://only.example.edu/sp")
        .unwrap();
    assert_eq!(only.service.availability, Some(Availability::IncommonOnly));

    let se = c.service_by_entity_id("https://sp.example.se/sp").unwrap();
    // eduGAIN's copy wins for another federation's registration (it carries the CoCo tag).
    assert_eq!(
        se.service.availability,
        Some(Availability::EdugainImportedByIncommon)
    );
    assert!(se
        .service
        .categories
        .contains(&"code-of-conduct-v2".to_owned()));
    assert!(se
        .service
        .notes
        .iter()
        .any(|n| n.contains("privacy statement")));
    // Trailing-slash registrar still joins to the federation list.
    assert_eq!(
        se.federation.as_ref().map(|f| f.code.as_str()),
        Some("SWAMID")
    );

    let fr = c.service_by_entity_id("https://sp.example.fr/sp").unwrap();
    assert_eq!(fr.service.availability, Some(Availability::EdugainOnly));
    assert_eq!(fr.name, "Exemple");
}

#[test]
fn search_filters_and_facets() {
    let c = catalog(None);
    let all = c.search(&ServiceQuery::default());
    assert_eq!(all.total, 4);

    let q = c.search(&ServiceQuery {
        q: Some("wiki".into()),
        ..Default::default()
    });
    assert_eq!(q.total, 1);
    assert_eq!(
        q.items[0].logo.as_deref(),
        Some("https://sp.example.edu/logo.png")
    );

    let us = c.search(&ServiceQuery {
        country: Some("us".into()),
        ..Default::default()
    });
    assert_eq!(us.total, 2);
    // The country facet ignores the country filter itself, so other choices stay visible.
    assert_eq!(us.facets["country"].len(), 3);

    let fed = c.search(&ServiceQuery {
        federation: Some("swamid".into()),
        ..Default::default()
    });
    assert_eq!(fed.total, 1);

    let attr = c.search(&ServiceQuery {
        attribute: Some("MAIL".into()),
        ..Default::default()
    });
    assert_eq!(attr.total, 1);

    let bad = c.search(&ServiceQuery {
        availability: Some("nope".into()),
        ..Default::default()
    });
    assert_eq!(bad.total, 0);

    let providers = c.providers(&Default::default());
    assert_eq!(providers.items[0].name, "Example University");
    assert_eq!(providers.items[0].service_count, 2);

    let feds = c.federations();
    let ic = feds
        .iter()
        .find(|f| f.federation.code == "INCOMMON")
        .unwrap();
    assert_eq!((ic.catalog_services, ic.catalog_identity_providers), (2, 1));
}

#[test]
fn curation_is_layered_and_can_hide() {
    let cur = CurationFile::from_json(
        br#"{"providers":{"example university":{"provider_type":"non-commercial","tags":["campus"]}},
             "services":{"https://only.example.edu/sp":{"hidden":true},
                         "https://sp.example.edu/shibboleth":{"tags":["wiki"],"summary":"Curated."}}}"#,
    )
    .unwrap();
    let c = catalog(Some(&cur));
    let wiki = c
        .service_by_entity_id("https://sp.example.edu/shibboleth")
        .unwrap();
    let cu = wiki.service.curated.unwrap();
    assert_eq!(cu.provider_type.as_deref(), Some("non-commercial"));
    assert_eq!(cu.tags, vec!["campus", "wiki"]);
    assert_eq!(c.search(&ServiceQuery::default()).total, 3);
    let pt = c.search(&ServiceQuery {
        provider_type: Some("Non-Commercial".into()),
        ..Default::default()
    });
    assert_eq!(pt.total, 1);
}

#[test]
fn clash_keeps_incommon_registration() {
    use catalog_core::model::Registration;
    let mk = |reg: &str| catalog_core::model::Service {
        id: catalog_core::entity_hash("https://vendor.example.com/sp"),
        entity_id: "https://vendor.example.com/sp".into(),
        registration: Registration {
            authority: Some(reg.into()),
            ..Default::default()
        },
        ..Default::default()
    };
    // Read order must not matter.
    for order in [
        [Source::Edugain, Source::Incommon],
        [Source::Incommon, Source::Edugain],
    ] {
        let mut b = Builder::new();
        for src in order {
            let reg = if src == Source::Incommon {
                "https://incommon.org"
            } else {
                "http://ukfederation.org.uk"
            };
            b.add_service(src, mk(reg));
        }
        let (services, _) = b.finish(&[], None);
        let s = &services[0];
        assert_eq!(s.registrar(), Some("https://incommon.org"));
        assert_eq!(s.also_registered_by, vec!["http://ukfederation.org.uk"]);
        assert_eq!(s.availability, Some(Availability::IncommonAndEdugain));
        assert!(s.notes.iter().any(|n| n.contains("clash")));
    }
}

#[test]
fn any_match_needs_half_the_words() {
    let c = catalog(None);
    let q = |text: &str, mode: Option<&str>| {
        c.search(&ServiceQuery {
            q: Some(text.into()),
            match_mode: mode.map(Into::into),
            ..Default::default()
        })
        .total
    };
    assert_eq!(q("wiki zebra", None), 0);
    assert_eq!(q("wiki zebra", Some("any")), 1);
    // Two of three words needed: "wiki" alone is not enough.
    assert_eq!(q("wiki zebra giraffe", Some("any")), 0);
    // Plurals find singulars.
    assert_eq!(q("wikis", None), 1);
}

#[test]
fn generated_keywords_find_services_and_test_copies_rank_lower() {
    use catalog_core::model::{Enrichment, Service, Snapshot};
    let svc = |id: &str, name: &str, kw: &[&str], env: &str| {
        let mut s = Service {
            entity_id: id.into(),
            id: catalog_core::entity_hash(id),
            ..Default::default()
        };
        s.display_names.insert("en".into(), name.into());
        s.enrichment = Some(Enrichment {
            summary: format!("{name}, a learning management system."),
            keywords: kw.iter().map(|k| k.to_string()).collect(),
            environment: env.into(),
            basis: "metadata_and_general_knowledge".into(),
            ..Default::default()
        });
        s
    };
    let c = Catalog::new(Snapshot {
        services: vec![
            svc(
                "https://a/",
                "Canvas Staging",
                &["lms", "course management"],
                "staging",
            ),
            svc(
                "https://b/",
                "Canvas",
                &["lms", "course management"],
                "production",
            ),
        ],
        ..Default::default()
    });
    let page = c.search(&ServiceQuery {
        q: Some("lms".into()),
        ..Default::default()
    });
    assert_eq!(page.total, 2, "generated keywords make 'lms' findable");
    assert_eq!(
        page.items[0].name, "Canvas",
        "production ranks above staging"
    );
    assert_eq!(
        page.items[0].description_source.as_deref(),
        Some("generated")
    );
    assert_eq!(page.items[1].environment.as_deref(), Some("staging"));
}
