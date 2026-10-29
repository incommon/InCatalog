// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Regions as named sets of ISO 3166 country codes, for "services in Europe" or
//! "anything from BRICS countries".
//!
//! Geographic regions follow UN M49, the UN Statistics Division's standard
//! grouping: continents, their subregions, and the intermediate regions of Africa
//! and the Americas, exactly as M49 lists them. So Cyprus and Turkey are in Western
//! Asia, Mexico is in Latin America rather than Northern America, and Greenland is
//! in Northern America. Areas M49 does not list (Taiwan, Kosovo) are in no
//! geographic region. "Asia-Pacific" is not an M49 region; it is defined here as a
//! union of M49 regions and says so.
//!
//! Political groupings change, so each carries the date its membership list took
//! effect. The EU is not "Europe".
//!
//! A federation that lists several countries is in a region when any of its
//! countries is, the same rule the `country` filter applies: WAYF (Denmark, Iceland,
//! Greenland) is in Europe, the EU, and Northern America. A federation that lists no
//! country is in no region.

use serde::Serialize;
use utoipa::ToSchema;

const M49: &str = "UN M49, https://unstats.un.org/unsd/methodology/m49/";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum RegionKind {
    /// A UN M49 region, or a union of them.
    Geographic,
    /// A membership grouping such as the EU, dated.
    Political,
}

#[derive(Debug)]
enum Members {
    Countries(&'static [&'static str]),
    /// The union of other regions, by key.
    Regions(&'static [&'static str]),
}

#[derive(Debug)]
pub struct Region {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: RegionKind,
    pub source: &'static str,
    /// For political groupings, the date the listed membership took effect.
    pub as_of: Option<&'static str>,
    /// How a natural-language query may name it, besides its label.
    pub aliases: &'static [&'static str],
    /// Said whenever a query is read as this region: how an ambiguous name was taken.
    pub reading_note: Option<&'static str>,
    members: Members,
}

macro_rules! m49 {
    ($key:literal, $label:literal, $aliases:expr, $members:expr) => {
        Region {
            key: $key,
            label: $label,
            kind: RegionKind::Geographic,
            source: M49,
            as_of: None,
            aliases: $aliases,
            reading_note: None,
            members: $members,
        }
    };
}

use Members::{Countries, Regions};

pub static REGIONS: &[Region] = &[
    // ---- Africa ----
    m49!(
        "africa",
        "Africa",
        &["african", "africa's"],
        Regions(&["northern-africa", "sub-saharan-africa"])
    ),
    m49!(
        "northern-africa",
        "Northern Africa",
        &["north africa", "north african"],
        Countries(&["DZ", "EG", "LY", "MA", "SD", "TN", "EH"])
    ),
    m49!(
        "sub-saharan-africa",
        "Sub-Saharan Africa",
        &["subsaharan africa", "sub-saharan"],
        Regions(&[
            "eastern-africa",
            "middle-africa",
            "southern-africa",
            "western-africa",
        ])
    ),
    m49!(
        "eastern-africa",
        "Eastern Africa",
        &["east africa", "east african"],
        Countries(&[
            "IO", "BI", "KM", "DJ", "ER", "ET", "TF", "KE", "MG", "MW", "MU", "YT", "MZ", "RE",
            "RW", "SC", "SO", "SS", "UG", "TZ", "ZM", "ZW",
        ])
    ),
    m49!(
        "middle-africa",
        "Middle Africa",
        &["central africa", "central african"],
        Countries(&["AO", "CM", "CF", "TD", "CG", "CD", "GQ", "GA", "ST"])
    ),
    m49!(
        "southern-africa",
        "Southern Africa",
        &[],
        Countries(&["BW", "SZ", "LS", "NA", "ZA"])
    ),
    m49!(
        "western-africa",
        "Western Africa",
        &["west africa", "west african"],
        Countries(&[
            "BJ", "BF", "CV", "CI", "GM", "GH", "GN", "GW", "LR", "ML", "MR", "NE", "NG", "SH",
            "SN", "SL", "TG",
        ])
    ),
    // ---- Americas ----
    m49!(
        "americas",
        "the Americas",
        &["the americas", "americas"],
        Regions(&["latin-america", "northern-america"])
    ),
    Region {
        key: "northern-america",
        label: "Northern America",
        kind: RegionKind::Geographic,
        source: M49,
        as_of: None,
        aliases: &["north america", "north american"],
        reading_note: Some("North America was read as UN M49 Northern America: the United States, Canada, Bermuda, Greenland, and Saint Pierre and Miquelon. Mexico and Central America are in Latin America."),
        members: Countries(&["BM", "CA", "GL", "PM", "US"]),
    },
    m49!(
        "latin-america",
        "Latin America and the Caribbean",
        &["latin america", "latin american", "latam"],
        Regions(&["caribbean", "central-america", "south-america"])
    ),
    m49!(
        "caribbean",
        "the Caribbean",
        &["caribbean"],
        Countries(&[
            "AI", "AG", "AW", "BS", "BB", "BQ", "VG", "KY", "CU", "CW", "DM", "DO", "GD", "GP",
            "HT", "JM", "MQ", "MS", "PR", "BL", "KN", "LC", "MF", "VC", "SX", "TT", "TC", "VI",
        ])
    ),
    m49!(
        "central-america",
        "Central America",
        &["central american"],
        Countries(&["BZ", "CR", "SV", "GT", "HN", "MX", "NI", "PA"])
    ),
    m49!(
        "south-america",
        "South America",
        &["south american"],
        Countries(&[
            "AR", "BO", "BV", "BR", "CL", "CO", "EC", "FK", "GF", "GY", "PY", "PE", "GS", "SR",
            "UY", "VE",
        ])
    ),
    // ---- Asia ----
    m49!(
        "asia",
        "Asia",
        &["asian"],
        Regions(&[
            "central-asia",
            "eastern-asia",
            "south-eastern-asia",
            "southern-asia",
            "western-asia",
        ])
    ),
    m49!(
        "central-asia",
        "Central Asia",
        &["central asian"],
        Countries(&["KZ", "KG", "TJ", "TM", "UZ"])
    ),
    m49!(
        "eastern-asia",
        "Eastern Asia",
        &["east asia", "east asian"],
        Countries(&["CN", "HK", "MO", "KP", "JP", "MN", "KR"])
    ),
    m49!(
        "south-eastern-asia",
        "South-eastern Asia",
        &["southeast asia", "south-east asia", "south east asia", "southeast asian"],
        Countries(&["BN", "KH", "ID", "LA", "MY", "MM", "PH", "SG", "TH", "TL", "VN"])
    ),
    m49!(
        "southern-asia",
        "Southern Asia",
        &["south asia", "south asian"],
        Countries(&["AF", "BD", "BT", "IN", "IR", "MV", "NP", "PK", "LK"])
    ),
    m49!(
        "western-asia",
        "Western Asia",
        &["west asia"],
        Countries(&[
            "AM", "AZ", "BH", "CY", "GE", "IQ", "IL", "JO", "KW", "LB", "OM", "QA", "SA", "PS",
            "SY", "TR", "AE", "YE",
        ])
    ),
    Region {
        key: "asia-pacific",
        label: "Asia-Pacific",
        kind: RegionKind::Geographic,
        source: "Not an M49 region: the union of M49 Eastern, South-eastern, and Southern Asia and Oceania",
        as_of: None,
        aliases: &["asia-pacific", "asia pacific", "apac"],
        reading_note: Some("Asia-Pacific has no standard definition; it was read as Eastern, South-eastern, and Southern Asia plus Oceania (UN M49)."),
        members: Regions(&["eastern-asia", "south-eastern-asia", "southern-asia", "oceania"]),
    },
    // ---- Europe ----
    m49!(
        "europe",
        "Europe",
        &["european", "europe's"],
        Regions(&[
            "eastern-europe",
            "northern-europe",
            "southern-europe",
            "western-europe",
        ])
    ),
    m49!(
        "eastern-europe",
        "Eastern Europe",
        &["east europe", "east european", "eastern european"],
        Countries(&["BY", "BG", "CZ", "HU", "PL", "MD", "RO", "RU", "SK", "UA"])
    ),
    m49!(
        "northern-europe",
        "Northern Europe",
        &["north europe", "northern european"],
        Countries(&[
            "AX", "DK", "EE", "FO", "FI", "GG", "IS", "IE", "IM", "JE", "LV", "LT", "NO", "SJ",
            "SE", "GB",
        ])
    ),
    m49!(
        "southern-europe",
        "Southern Europe",
        &["south europe", "southern european"],
        Countries(&[
            "AL", "AD", "BA", "HR", "GI", "GR", "VA", "IT", "MT", "ME", "MK", "PT", "SM", "RS",
            "SI", "ES",
        ])
    ),
    m49!(
        "western-europe",
        "Western Europe",
        &["west europe", "western european"],
        Countries(&["AT", "BE", "FR", "DE", "LI", "LU", "MC", "NL", "CH"])
    ),
    // ---- Oceania ----
    m49!(
        "oceania",
        "Oceania",
        &["oceanian", "pacific islands"],
        Regions(&[
            "australia-and-new-zealand",
            "melanesia",
            "micronesia",
            "polynesia",
        ])
    ),
    m49!(
        "australia-and-new-zealand",
        "Australia and New Zealand",
        &["australasia", "australia and new zealand"],
        Countries(&["AU", "CX", "CC", "HM", "NZ", "NF"])
    ),
    m49!(
        "melanesia",
        "Melanesia",
        &[],
        Countries(&["FJ", "NC", "PG", "SB", "VU"])
    ),
    m49!(
        "micronesia",
        "Micronesia",
        &[],
        Countries(&["GU", "KI", "MH", "FM", "NR", "MP", "PW", "UM"])
    ),
    m49!(
        "polynesia",
        "Polynesia",
        &[],
        Countries(&["AS", "CK", "PF", "NU", "PN", "WS", "TK", "TO", "TV", "WF"])
    ),
    // ---- Political groupings ----
    Region {
        key: "eu",
        label: "the European Union",
        kind: RegionKind::Political,
        source: "EU member states, https://european-union.europa.eu/principles-countries-history/eu-countries_en",
        // The United Kingdom's withdrawal; no state has joined since.
        as_of: Some("2020-02-01"),
        aliases: &["the eu", "eu", "european union", "the european union", "eu member states", "eu countries"],
        reading_note: None,
        members: Countries(EU),
    },
    Region {
        key: "eea",
        label: "the European Economic Area",
        kind: RegionKind::Political,
        source: "The EU plus Iceland, Liechtenstein, and Norway, https://www.efta.int/eea",
        as_of: Some("2020-02-01"),
        aliases: &["the eea", "eea", "european economic area", "the european economic area"],
        reading_note: None,
        members: Regions(&["eu", "eea-efta"]),
    },
    Region {
        key: "eea-efta",
        label: "the EEA EFTA states",
        kind: RegionKind::Political,
        source: "EFTA members in the EEA, https://www.efta.int/eea",
        as_of: Some("1995-05-01"),
        aliases: &["eea efta"],
        reading_note: None,
        members: Countries(&["IS", "LI", "NO"]),
    },
    Region {
        key: "brics",
        label: "BRICS",
        kind: RegionKind::Political,
        source: "BRICS full members, https://en.wikipedia.org/wiki/Member_states_of_BRICS",
        // Indonesia joined. Saudi Arabia was invited in 2023 and has not confirmed
        // membership, so it is not listed.
        as_of: Some("2025-01-06"),
        aliases: &["brics", "brics countries", "brics nations", "brics members"],
        reading_note: None,
        members: Countries(&["BR", "RU", "IN", "CN", "ZA", "EG", "ET", "IR", "AE", "ID"]),
    },
];

const EU: &[&str] = &[
    "AT", "BE", "BG", "HR", "CY", "CZ", "DK", "EE", "FI", "FR", "DE", "GR", "HU", "IE", "IT", "LV",
    "LT", "LU", "MT", "NL", "PL", "PT", "RO", "SK", "SI", "ES", "SE",
];

impl Region {
    /// Its ISO 3166 alpha-2 codes, sorted, without repeats.
    pub fn countries(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = match &self.members {
            Members::Countries(cs) => cs.to_vec(),
            Members::Regions(keys) => keys
                .iter()
                .filter_map(|k| get(k))
                .flat_map(|r| r.countries())
                .collect(),
        };
        out.sort_unstable();
        out.dedup();
        out
    }

    pub fn expansion(&self) -> RegionExpansion {
        RegionExpansion {
            key: self.key.to_owned(),
            label: self.label.to_owned(),
            kind: self.kind,
            source: self.source.to_owned(),
            as_of: self.as_of.map(str::to_owned),
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
    /// For political groupings, the date the listed membership took effect.
    pub as_of: Option<String>,
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
        for r in REGIONS {
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
            if let Members::Regions(keys) = &r.members {
                for k in *keys {
                    assert!(get(k).is_some(), "{} names a missing region {k}", r.key);
                }
            }
            assert_eq!(
                r.as_of.is_some(),
                r.kind == RegionKind::Political,
                "political groupings are dated, geographic regions are not: {}",
                r.key
            );
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
        assert_eq!(brics.as_of, Some("2025-01-06"));
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
}
