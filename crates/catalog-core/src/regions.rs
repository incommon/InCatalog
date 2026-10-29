// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Regions and groupings as named sets of ISO 3166 country codes, for "services in
//! Europe", "anything from BRICS countries", or "NATO members".
//!
//! They live as data in `data/regions.json`, each with its source and the date it
//! was checked, so a membership change is an edit there, not here. A test fails
//! once an entry was checked more than a year ago, as a prompt to recheck it.
//!
//! - **Geographic regions** follow UN M49, the UN Statistics Division's standard
//!   grouping: continents, their subregions, and the intermediate regions of Africa
//!   and the Americas, exactly as M49 lists them. So Cyprus and Turkey are in
//!   Western Asia, Mexico is in Latin America rather than Northern America, and
//!   Greenland is in Northern America. Areas M49 does not list (Taiwan, Kosovo) are
//!   in no geographic region. "Asia-Pacific" is not an M49 region; it is defined as
//!   a union of M49 regions and says so.
//! - **Political, security, and economic groupings** (the EU, Schengen, the euro
//!   area, NATO, the OECD, ASEAN, BRICS, …) carry the date their listed membership
//!   took effect, when recent members joined, and when former members left. The EU
//!   is not "Europe", and Schengen is not the EU. Africa's regional economic
//!   communities, currency and customs unions, AfCFTA, and the Arab League are here
//!   too; memberships overlap (Kenya is in the EAC, COMESA, and IGAD).
//! - **Research and education networks** (GÉANT, RedCLARA, APAN, WACREN,
//!   UbuntuNet, ASREN) stand for the countries of their member networks.
//!
//! Groupings whose membership is contested, or that label countries by a judgment
//! (sanctions lists, "developed countries"), are left out on purpose. Where a list
//! had a choice to make (G20's EU and AU seats, the AU's Sahrawi Republic,
//! Mercosur's suspended Venezuela), its `note` says what was chosen.
//!
//! A federation that lists several countries is in a region when any of its
//! countries is, the same rule the `country` filter applies: WAYF (Denmark, Iceland,
//! Greenland) is in Europe, the EU, and Northern America. A federation that lists no
//! country is in no region.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum RegionKind {
    /// A UN M49 region, or a union of them.
    Geographic,
    /// A treaty, security, or economic grouping, dated.
    Political,
    /// The countries of a research and education network's member networks.
    Network,
}

/// One entry of `data/regions.json`.
#[derive(Debug, Deserialize)]
pub struct Region {
    pub key: String,
    pub label: String,
    pub kind: RegionKind,
    pub source: String,
    pub url: String,
    /// When the list was last checked against its source.
    pub checked: String,
    /// For groupings, the date the listed membership took effect.
    #[serde(default)]
    pub as_of: Option<String>,
    /// How a natural-language query may name it, besides its label.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Said whenever a query is read as this region: how an ambiguous name was taken.
    #[serde(default)]
    pub reading_note: Option<String>,
    /// What the list includes or leaves out, where that took a choice.
    #[serde(default)]
    pub note: Option<String>,
    /// When recent members joined, by country code.
    #[serde(default)]
    pub joined: BTreeMap<String, String>,
    /// Former members and when they left, by country code. They are not in
    /// `countries`; recording them says the list was not simply left short.
    #[serde(default)]
    pub left: BTreeMap<String, String>,
    #[serde(default)]
    countries: Vec<String>,
    /// Other regions, by key, whose countries this one also includes.
    #[serde(default)]
    regions: Vec<String>,
}

#[derive(Deserialize)]
struct File {
    regions: Vec<Region>,
}

/// Every region, in the order of `data/regions.json`.
pub static REGIONS: LazyLock<Vec<Region>> = LazyLock::new(|| {
    serde_json::from_str::<File>(include_str!("../data/regions.json"))
        .expect("data/regions.json is valid")
        .regions
});

impl Region {
    /// Its ISO 3166 alpha-2 codes, sorted, without repeats.
    pub fn countries(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.countries.iter().map(String::as_str).collect();
        for key in &self.regions {
            if let Some(r) = get(key) {
                out.extend(r.countries());
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    pub fn expansion(&self) -> RegionExpansion {
        RegionExpansion {
            key: self.key.clone(),
            label: self.label.clone(),
            kind: self.kind,
            source: self.source.clone(),
            url: self.url.clone(),
            checked: self.checked.clone(),
            as_of: self.as_of.clone(),
            note: self.note.clone(),
            joined: self.joined.clone(),
            left: self.left.clone(),
            countries: self.countries().into_iter().map(str::to_owned).collect(),
        }
    }
}

/// A region and the countries it stands for, as a search used it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct RegionExpansion {
    pub key: String,
    pub label: String,
    pub kind: RegionKind,
    /// Where the definition comes from.
    pub source: String,
    pub url: String,
    /// When the list was last checked against its source.
    pub checked: String,
    /// For groupings, the date the listed membership took effect.
    pub as_of: Option<String>,
    /// What the list includes or leaves out, where that took a choice.
    pub note: Option<String>,
    /// When recent members joined, by country code.
    pub joined: BTreeMap<String, String>,
    /// Former members and when they left, by country code.
    pub left: BTreeMap<String, String>,
    /// ISO 3166 alpha-2 codes, sorted.
    pub countries: Vec<String>,
}

pub fn get(key: &str) -> Option<&'static Region> {
    REGIONS.iter().find(|r| r.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn countries(key: &str) -> Vec<&'static str> {
        get(key).unwrap().countries()
    }

    #[test]
    fn every_region_resolves_and_keys_are_unique() {
        for r in REGIONS.iter() {
            assert!(!r.countries().is_empty(), "{}", r.key);
            assert_eq!(
                REGIONS.iter().filter(|o| o.key == r.key).count(),
                1,
                "{}",
                r.key
            );
            for cc in r.countries() {
                assert!(
                    cc.len() == 2 && cc.chars().all(|c| c.is_ascii_uppercase()),
                    "{}: {cc}",
                    r.key
                );
            }
            for k in &r.regions {
                assert!(get(k).is_some(), "{} names a missing region {k}", r.key);
            }
            for cc in r.joined.keys() {
                assert!(
                    r.countries().contains(&cc.as_str()),
                    "{}: {cc} joined but is not listed",
                    r.key
                );
            }
            for cc in r.left.keys() {
                assert!(
                    !r.countries().contains(&cc.as_str()),
                    "{}: {cc} left but is still listed",
                    r.key
                );
            }
            assert_eq!(
                r.as_of.is_some(),
                r.kind != RegionKind::Geographic,
                "groupings are dated, geographic regions are not: {}",
                r.key
            );
            assert!(r.url.starts_with("https://"), "{}", r.key);
        }
    }

    #[test]
    fn m49_continents_do_not_overlap() {
        let continents = ["africa", "americas", "asia", "europe", "oceania"];
        let mut seen = std::collections::HashMap::new();
        for c in continents {
            for cc in countries(c) {
                if let Some(other) = seen.insert(cc, c) {
                    panic!("{cc} is in both {other} and {c}");
                }
            }
        }
        // 249 ISO codes, less Antarctica, Taiwan, and the few M49 leaves unplaced.
        assert!(seen.len() > 240, "{}", seen.len());
    }

    #[test]
    fn expansions_are_the_unions_of_their_parts() {
        assert_eq!(countries("africa").len(), 7 + 22 + 9 + 5 + 17);
        assert_eq!(countries("americas").len(), 28 + 8 + 16 + 5);
        assert_eq!(countries("europe").len(), 10 + 16 + 16 + 9);
        assert_eq!(countries("eu").len(), 27);
        assert_eq!(countries("eea").len(), 30);
        assert_eq!(countries("brics").len(), 10);
        assert!(countries("asia-pacific").contains(&"AU"));
        assert!(!countries("asia-pacific").contains(&"TR"));
    }

    #[test]
    fn the_eu_is_not_europe() {
        let europe = countries("europe");
        let eu = countries("eu");
        // Cyprus is an EU member in Western Asia; Norway, Switzerland, and the UK are
        // in Europe and not in the EU.
        assert!(eu.contains(&"CY") && !europe.contains(&"CY"));
        for cc in ["NO", "CH", "GB"] {
            assert!(europe.contains(&cc) && !eu.contains(&cc), "{cc}");
        }
        assert!(countries("western-asia").contains(&"TR"));
        assert!(countries("latin-america").contains(&"MX"));
        assert!(!countries("northern-america").contains(&"MX"));
        assert!(countries("northern-america").contains(&"GL"));
    }

    #[test]
    fn membership_lists_are_dated() {
        let brics = get("brics").unwrap();
        assert_eq!(brics.as_of.as_deref(), Some("2025-01-06"));
        for cc in ["BR", "RU", "IN", "CN", "ZA", "EG", "ET", "IR", "AE", "ID"] {
            assert!(brics.countries().contains(&cc), "{cc}");
        }
        assert!(!brics.countries().contains(&"SA"), "invited, not confirmed");
        assert!(!brics.countries().contains(&"GB"));
        let eu = get("eu").unwrap().expansion();
        assert_eq!(eu.as_of.as_deref(), Some("2020-02-01"));
        assert_eq!(eu.kind, RegionKind::Political);
        assert!(!eu.countries.contains(&"GB".to_owned()));
    }

    /// Membership moves, so every list is rechecked at least once a year. When this
    /// fails, check the entry against its `url`, update it in `data/regions.json`,
    /// and set `checked` to today.
    #[test]
    fn every_list_was_checked_within_a_year() {
        let today = chrono::Utc::now().date_naive();
        for r in REGIONS.iter() {
            let checked = chrono::NaiveDate::parse_from_str(&r.checked, "%Y-%m-%d")
                .unwrap_or_else(|e| panic!("{}: checked {:?}: {e}", r.key, r.checked));
            assert!(
                (today - checked).num_days() <= 365,
                "{} was last checked on {}; recheck it against {}",
                r.key,
                r.checked,
                r.url
            );
            assert!(checked <= today, "{} is checked in the future", r.key);
        }
    }

    #[test]
    fn groupings_are_not_each_other() {
        let schengen = countries("schengen");
        let eu = countries("eu");
        let euro = countries("eurozone");
        // Schengen is not the EU, and neither is the euro area.
        for cc in ["NO", "CH", "IS", "LI"] {
            assert!(schengen.contains(&cc) && !eu.contains(&cc), "{cc}");
        }
        for cc in ["IE", "CY"] {
            assert!(eu.contains(&cc) && !schengen.contains(&cc), "{cc}");
        }
        assert!(!euro.contains(&"SE") && !euro.contains(&"PL") && euro.contains(&"BG"));
        let lens: Vec<(&str, usize)> = [
            "schengen",
            "eurozone",
            "nato",
            "oecd",
            "g7",
            "g20",
            "asean",
            "apec",
            "mercosur",
            "usmca",
            "african-union",
            "gcc",
            "commonwealth",
            "five-eyes",
            "nordic-council",
        ]
        .into_iter()
        .map(|k| (k, countries(k).len()))
        .collect();
        assert_eq!(
            lens,
            [
                ("schengen", 29),
                ("eurozone", 21),
                ("nato", 32),
                ("oecd", 38),
                ("g7", 7),
                ("g20", 19),
                ("asean", 11),
                ("apec", 21),
                ("mercosur", 5),
                ("usmca", 3),
                ("african-union", 54),
                ("gcc", 6),
                ("commonwealth", 56),
                ("five-eyes", 5),
                ("nordic-council", 8),
            ]
        );
    }

    #[test]
    fn recent_joins_are_dated() {
        let joined = |k: &str, cc: &str| get(k).unwrap().joined.get(cc).cloned();
        assert_eq!(joined("nato", "FI").as_deref(), Some("2023-04-04"));
        assert_eq!(joined("nato", "SE").as_deref(), Some("2024-03-07"));
        assert_eq!(joined("schengen", "RO").as_deref(), Some("2025-01-01"));
        assert_eq!(joined("eurozone", "BG").as_deref(), Some("2026-01-01"));
        assert_eq!(joined("asean", "TL").as_deref(), Some("2025-10-26"));
        let e = get("nato").unwrap().expansion();
        assert_eq!(
            (e.kind, e.as_of.as_deref()),
            (RegionKind::Political, Some("2024-03-07"))
        );
        assert_eq!(e.joined.len(), 2);
    }

    #[test]
    fn choices_are_written_down() {
        // The AU's Sahrawi Republic is not equated with Western Sahara; Mercosur's
        // suspended Venezuela and Kosovo's network are left out, and each says so.
        assert!(!countries("african-union").contains(&"EH"));
        assert!(!countries("mercosur").contains(&"VE"));
        for k in [
            "african-union",
            "mercosur",
            "g20",
            "geant",
            "brics",
            "asren",
        ] {
            assert!(get(k).unwrap().note.is_some(), "{k}");
        }
        let net = get("geant").unwrap();
        assert_eq!((net.kind, net.countries().len()), (RegionKind::Network, 42));
        for k in ["redclara", "apan", "wacren", "ubuntunet", "asren"] {
            assert_eq!(get(k).unwrap().kind, RegionKind::Network, "{k}");
        }
    }

    #[test]
    fn african_blocs_record_who_left_and_overlap() {
        let ecowas = get("ecowas").unwrap();
        assert_eq!(ecowas.countries().len(), 12);
        for cc in ["BF", "ML", "NE"] {
            assert_eq!(
                ecowas.left.get(cc).map(String::as_str),
                Some("2025-01-29"),
                "{cc}"
            );
            // They left ECOWAS but are still in the West African CFA franc zone.
            assert!(countries("waemu").contains(&cc), "{cc}");
        }
        assert_eq!(
            get("igad").unwrap().left.get("ER").map(String::as_str),
            Some("2025-12-12")
        );
        assert_eq!(ecowas.expansion().left.len(), 3);

        // Kenya is in three blocs at once.
        for k in ["eac", "comesa", "igad", "afcfta"] {
            assert!(countries(k).contains(&"KE"), "{k}");
        }
        assert!(!countries("sadc").contains(&"KE"));

        let lens: Vec<(&str, usize)> = [
            "sadc",
            "eac",
            "comesa",
            "eccas",
            "igad",
            "cen-sad",
            "amu",
            "waemu",
            "cemac",
            "cfa-franc-zone",
            "sacu",
            "afcfta",
            "arab-league",
        ]
        .into_iter()
        .map(|k| (k, countries(k).len()))
        .collect();
        assert_eq!(
            lens,
            [
                ("sadc", 16),
                ("eac", 8),
                ("comesa", 21),
                ("eccas", 11),
                ("igad", 7),
                ("cen-sad", 25),
                ("amu", 5),
                ("waemu", 8),
                ("cemac", 6),
                ("cfa-franc-zone", 14),
                ("sacu", 5),
                ("afcfta", 49),
                ("arab-league", 22),
            ]
        );
        // The Arab League spans Africa and Asia.
        assert!(
            countries("arab-league").contains(&"SA") && countries("arab-league").contains(&"EG")
        );
        assert!(get("afcfta")
            .unwrap()
            .note
            .as_deref()
            .unwrap()
            .contains("not every signatory"));
    }
}
