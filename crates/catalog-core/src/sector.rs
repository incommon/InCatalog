// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Who runs a service: the sector of the organization that registered it (a
//! university, a library, a vendor, a government agency, …), not of the people it
//! is for. An organization can be in more than one: a university medical center is
//! `academia` and `healthcare`, and a publisher that is a company is `publishers`
//! and `commercial`.
//!
//! Metadata has no sector, so it is read in this order, and the result records which
//! decided:
//!
//! 1. **Curated** (`curated`): a curation overlay's `provider_type`, mapped onto the
//!    vocabulary.
//! 2. **Rules** (`metadata`): the organization's name and website domain. `.edu`
//!    Names like "University", "Universität", "Université", "Hochschule" are
//!    `academia` (an `.edu` or `.ac.*` domain alone is only a hint: Jisc, EDUCAUSE,
//!    and HighWire Press hold them too); `.gov`, `.gouv.*`, `.gob.*`, `.govt.*` domains and
//!    "Ministry", "Bundesamt" are `government`; a federation's or research network's
//!    own domain is `nren`; and so on. The rules are narrow, because they outrank
//!    Claude: "Cambridge University Press" is a publisher, not a university; an
//!    "Online Library" or "Ebook Library" is a product, not a library; and a company
//!    ("… GmbH", "… LLC") is not a museum, a hospital, or an agency because of a word
//!    in its name. "Inc." and "Ltd" do not decide `commercial`, since nonprofits use
//!    them too.
//! 3. **Claude** (`generated`): ingest asks once per organization, with the
//!    organization's name, website, and services, and these rules' weaker hints.
//! 4. Otherwise no sector (`unknown`).
//!
//! **Publicly funded institutions** are labeled by what they are, not who funds
//! them: a public university is `academia`, a public library `libraries`, a national
//! laboratory `research` (and `government` too when it runs on a `.gov` domain). A
//! vendor that sells to governments is `commercial`.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::curation::provider_name;
use crate::model::{Federation, Service};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Sector {
    /// Universities and colleges: higher education.
    Academia,
    /// Laboratories, research institutes, research infrastructures, collaborations.
    Research,
    /// Companies and vendors.
    Commercial,
    Libraries,
    /// Museums, archives, and galleries: cultural heritage.
    Museums,
    /// National, regional, and local government agencies and public bodies.
    Government,
    /// Hospitals and health systems.
    Healthcare,
    /// Schools and school districts.
    K12,
    Publishers,
    /// Foundations, associations, societies, and other nonprofits not listed above.
    Nonprofit,
    /// Federations and research and education networks.
    Nren,
}

impl Sector {
    pub const ALL: [Sector; 11] = [
        Sector::Academia,
        Sector::Research,
        Sector::Commercial,
        Sector::Libraries,
        Sector::Museums,
        Sector::Government,
        Sector::Healthcare,
        Sector::K12,
        Sector::Publishers,
        Sector::Nonprofit,
        Sector::Nren,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Sector::Academia => "academia",
            Sector::Research => "research",
            Sector::Commercial => "commercial",
            Sector::Libraries => "libraries",
            Sector::Museums => "museums",
            Sector::Government => "government",
            Sector::Healthcare => "healthcare",
            Sector::K12 => "k12",
            Sector::Publishers => "publishers",
            Sector::Nonprofit => "nonprofit",
            Sector::Nren => "nren",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Sector::Academia => "Universities and colleges",
            Sector::Research => "Research institutions",
            Sector::Commercial => "Companies",
            Sector::Libraries => "Libraries",
            Sector::Museums => "Museums, archives, and galleries",
            Sector::Government => "Government",
            Sector::Healthcare => "Healthcare",
            Sector::K12 => "Schools (K-12)",
            Sector::Publishers => "Publishers",
            Sector::Nonprofit => "Nonprofits",
            Sector::Nren => "Federations and networks",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == k)
    }
}

/// A service's sectors and what decided them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SectorReading {
    /// Empty when nothing decided (`unknown`).
    pub sectors: Vec<Sector>,
    /// `curated`, `metadata` (the rules), or `generated` (Claude); absent when
    /// nothing decided.
    pub source: Option<String>,
    /// What the rules matched, or Claude's reason.
    pub reason: Option<String>,
    /// For a generated reading.
    pub model: Option<String>,
    pub generated_at: Option<String>,
}

/// What the rules make of an organization: the sectors they decide, with why, and
/// weaker hints that do not decide but are passed to Claude.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rules {
    pub sectors: Vec<(Sector, String)>,
    pub hints: Vec<String>,
}

/// The website hosts of the federations and networks that run them, without
/// `www.`, from the eduGAIN federation list: an organization on one of them is a
/// federation or network operator.
pub fn federation_hosts(federations: &[Federation]) -> HashSet<String> {
    federations
        .iter()
        .flat_map(|f| [f.url.as_deref(), Some(f.registration_authority.as_str())])
        .flatten()
        .filter_map(host_of)
        .collect()
}

/// The organization's website host, lowercase, without `www.`.
pub fn organization_host(s: &Service) -> Option<String> {
    s.organization.urls.values().find_map(|u| host_of(u))
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.trim().split_once("://")?.1;
    let host = rest
        .split(['/', '?', '#', ':'])
        .next()?
        .to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_owned();
    host.contains('.').then_some(host)
}

/// A curated `provider_type`, free text, onto the vocabulary.
pub fn from_curated(provider_type: &str) -> Vec<Sector> {
    let t = provider_type.trim().to_lowercase();
    let mut out = Vec::new();
    let mut add = |s| {
        if !out.contains(&s) {
            out.push(s)
        }
    };
    for part in t.split([',', ';', '/', '+']).map(str::trim) {
        match part {
            "commercial" | "vendor" | "company" | "for-profit" | "for profit" => {
                add(Sector::Commercial)
            }
            "non-commercial" | "noncommercial" | "nonprofit" | "non-profit" | "not-for-profit"
            | "charity" => add(Sector::Nonprofit),
            "government" | "public sector" | "governmental" | "public body" => {
                add(Sector::Government)
            }
            "academic" | "academia" | "university" | "higher education" | "college" => {
                add(Sector::Academia)
            }
            "research" | "research institute" | "laboratory" => add(Sector::Research),
            "library" | "libraries" => add(Sector::Libraries),
            "museum" | "museums" | "archive" | "archives" | "glam" | "cultural heritage" => {
                add(Sector::Museums)
            }
            "healthcare" | "health" | "hospital" => add(Sector::Healthcare),
            "k12" | "k-12" | "school" | "schools" => add(Sector::K12),
            "publisher" | "publishers" | "publishing" => add(Sector::Publishers),
            "nren" | "federation" | "network" => add(Sector::Nren),
            _ => {}
        }
    }
    out
}

/// Phrases that name a university, college, or other higher-education institution,
/// in the languages the catalog's names use. Whole words, case-insensitively; a
/// `*` at the end matches any ending ("Hochschule" in "Fachhochschule" is handled by
/// [`ACADEMIA_SUFFIXES`]).
const ACADEMIA: &[&str] = &[
    "university",
    "universities",
    "universität",
    "universitat",
    "université",
    "universite",
    "universidad",
    "università",
    "universiteit",
    "universidade",
    "universitet",
    "uniwersytet",
    "univerzita",
    "universitas",
    "universiti",
    "college",
    "hochschule",
    "polytechnic",
    "politecnico",
    "politécnica",
    "yliopisto",
    "egyetem",
    "högskola",
    "hogeschool",
    "école",
    "ecole",
    "institute of technology",
];
const ACADEMIA_SUFFIXES: &[&str] = &["hochschule", "högskolan", "universität"];
/// Names that contain an academia word without being a university.
const NOT_ACADEMIA: &[&str] = &[
    "university press",
    "college board",
    "royal college of",
    "college of american",
];
const RESEARCH: &[&str] = &[
    "laboratory",
    "laboratories",
    "national lab",
    "research institute",
    "research center",
    "research centre",
    "forschungszentrum",
    "observatory",
    "scientific collaboration",
    "eric",
    "max planck",
    "max-planck",
    "fraunhofer",
    "cern",
    "cnrs",
    "research infrastructure",
    "research council",
];
/// "Library" says an organization is one only with a word that says what kind, or
/// as the plural ("Center for Research Libraries").
const LIBRARY_KINDS: &[&str] = &[
    "national",
    "university",
    "state",
    "public",
    "research",
    "city",
    "county",
    "central",
    "royal",
    "college",
    "school",
    "academic",
    "medical",
    "law",
    "congress",
];
const LIBRARY_PRODUCTS: &[&str] = &[
    "online",
    "electronic",
    "digital",
    "ebook",
    "computer",
    "genesis",
];
const LIBRARY_WORDS: &[&str] = &[
    "bibliothek",
    "biblioteca",
    "bibliothèque",
    "bibliotheque",
    "bibliotheek",
    "bibliotek",
    "kirjasto",
    "knihovna",
    "biblioteka",
    "libraries",
];
const MUSEUMS: &[&str] = &[
    "museum",
    "museums",
    "museo",
    "musée",
    "musee",
    "museu",
    "muzeum",
    "gallery",
    "galleria",
    "archives",
    "archiv",
    "archivo",
    "archivio",
    "national archives",
];
const GOVERNMENT: &[&str] = &[
    "ministry",
    "ministerium",
    "ministère",
    "ministere",
    "ministerio",
    "ministero",
    "bundesamt",
    "government",
    "agency",
    "department of",
    "county council",
    "city council",
    "national institutes of health",
];
const HEALTHCARE: &[&str] = &[
    "hospital",
    "hôpital",
    "hospitals",
    "krankenhaus",
    "klinikum",
    "medical center",
    "medical centre",
    "health system",
    "healthcare",
    "health care",
    "nhs",
];
const K12_WORDS: &[&str] = &[
    "school district",
    "public schools",
    "high school",
    "elementary school",
    "primary school",
    "secondary school",
    "unified school",
    "k 12",
    "k12",
];
const PUBLISHERS: &[&str] = &[
    "press",
    "publishing",
    "publishers",
    "publisher",
    "verlag",
    "éditions",
    "editions",
];
const NOT_PUBLISHER: &[&str] = &["publishing platform"];
const NONPROFIT: &[&str] = &[
    "foundation",
    "stiftung",
    "fondation",
    "fundación",
    "fundacion",
    "fondazione",
    "fundação",
    "association",
    "asociación",
    "associação",
    "society",
    "e.v.",
    "e. v.",
    "charity",
    "charitable",
];
const NREN: &[&str] = &[
    "research and education network",
    "research & education network",
    "academic network",
    "identity federation",
    "access federation",
    "access management federation",
    "nren",
];
/// Organizations that are federations or networks, by their whole name.
const NREN_NAMES: &[&str] = &[
    "internet2",
    "edugain",
    "eduroam",
    "geant",
    "géant",
    "géant association",
    "geant association",
    "jisc",
    "jisc services limited",
    "surf",
    "surf bv",
    "switch",
    "aarnet",
    "canarie",
    "renater",
    "garr",
    "dfn",
    "sunet",
    "nordunet",
    "cesnet",
    "rediris",
    "heanet",
    "belnet",
    "funet",
    "sikt",
    "reannz",
    "rnp",
    "cudi",
    "reuna",
    "kenet",
    "tenet",
    "ubuntunet alliance",
    "wacren",
    "asren",
    "apan",
    "redclara",
    "restena",
    "restena foundation",
    "arnes",
];

/// Legal forms only companies take.
const COMPANY: &[&str] = &[
    "llc",
    "l.l.c.",
    "gmbh",
    "ag",
    "b.v.",
    "bv",
    "plc",
    "s.a.",
    "s.l.",
    "s.r.l.",
    "srl",
    "pty",
    "oy",
    "a/s",
    "kk",
    "ltda",
    "n.v.",
    "sas",
    "s.a.s.",
    "co., ltd",
    "co.,ltd",
    "co. ltd",
    "pte. ltd",
    "pte ltd",
    "pvt ltd",
    "pvt. ltd",
    "company limited",
    "limited company",
    "holdings",
];
/// Legal forms nonprofits share with companies: a hint only.
/// "Corporation" too: the British Broadcasting Corporation is a public body.
const MAYBE_COMPANY: &[&str] = &[
    "inc",
    "inc.",
    "ltd",
    "ltd.",
    "limited",
    "company",
    "corp",
    "corp.",
    "corporation",
];

/// `name` lowercased, with punctuation that separates words turned into spaces and
/// spaces around the whole, so `contains(" word ")` finds whole words. Dots stay
/// ("e. V.", "Inc."); [`rules`] also looks at a copy without them.
fn padded(name: &str) -> String {
    let mut out = String::from(" ");
    for c in name.to_lowercase().chars() {
        let c = if c.is_whitespace()
            || matches!(
                c,
                ',' | '(' | ')' | '-' | '–' | '—' | '|' | ':' | ';' | '"' | '/'
            ) {
            ' '
        } else {
            c
        };
        if !(c == ' ' && out.ends_with(' ')) {
            out.push(c);
        }
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

/// The first of `words` that appears in `text` as whole words.
fn find<'a>(text: &str, words: &[&'a str]) -> Option<&'a str> {
    words
        .iter()
        .copied()
        .find(|w| text.contains(&format!(" {} ", padded(w).trim())))
}

/// The rules over an organization's name and website host.
pub fn rules(name: &str, host: Option<&str>, federation_hosts: &HashSet<String>) -> Rules {
    let text = padded(name);
    let plain = padded(&text.replace('.', " "));
    let host = host.unwrap_or("");
    let mut r = Rules::default();
    let add = |s: Sector, why: String, r: &mut Rules| {
        if !r.sectors.iter().any(|(x, _)| *x == s) {
            r.sectors.push((s, why));
        }
    };
    let company = find(&text, COMPANY).or_else(|| find(&plain, COMPANY));
    // Any legal form of a company, nonprofits' included, stops a word in the name
    // from making it a library, museum, hospital, agency, or lab.
    let incorporated = company.is_some() || find(&plain, MAYBE_COMPANY).is_some();
    let quoted = |w: &str| format!("\"{w}\" in the organization's name");

    // Publishers first: "University Press" is one, and not a university.
    if let Some(w) = find(&plain, PUBLISHERS).filter(|_| find(&plain, NOT_PUBLISHER).is_none()) {
        add(Sector::Publishers, quoted(w), &mut r);
    }
    let not_academia = find(&plain, NOT_ACADEMIA).is_some();
    let academia_word = find(&plain, ACADEMIA).or_else(|| {
        plain
            .split(' ')
            .find(|w| {
                ACADEMIA_SUFFIXES
                    .iter()
                    .any(|s| w.ends_with(s) && w.len() > s.len())
            })
            .map(|_| "hochschule")
    });
    // An academic domain alone does not decide: networks, consortia, labs, and
    // presses hold .edu and .ac.* domains too (Jisc, EDUCAUSE, the Center for
    // Research Libraries, HighWire Press). It is a hint for Claude.
    let academia = match academia_word {
        Some(w) if !not_academia => Some(quoted(w)),
        _ => None,
    };
    let academic_domain = host.ends_with(".edu") || is_academic_domain(host);
    if let Some(why) = &academia {
        add(Sector::Academia, why.clone(), &mut r);
    }
    if academia.is_none() && !incorporated {
        let compound = plain
            .split(' ')
            .find(|w| w.ends_with("forschungszentrum") && w.len() > "forschungszentrum".len());
        if let Some(w) = find(&plain, RESEARCH).or(compound) {
            add(Sector::Research, quoted(w), &mut r);
        }
    }
    if !incorporated {
        let words: Vec<&str> = plain.split(' ').filter(|w| !w.is_empty()).collect();
        let library = words.iter().enumerate().find_map(|(i, w)| {
            let kind = i.checked_sub(1).map(|j| words[j]);
            let after = words.get(i + 1).copied();
            // German, Dutch, and Nordic compounds: "Staatsbibliothek", "Universitetsbibliotek".
            let compound = ["bibliothek", "bibliotek"]
                .iter()
                .any(|s| w.ends_with(s) && w.len() > s.len());
            let is_library = compound
                || (*w == "library"
                    && (kind.is_some_and(|k| LIBRARY_KINDS.contains(&k)) || after == Some("of"))
                    && !kind.is_some_and(|k| LIBRARY_PRODUCTS.contains(&k)))
                || LIBRARY_WORDS.contains(w);
            is_library.then_some(*w)
        });
        if let Some(w) = library.filter(|_| !LIBRARY_PRODUCTS.iter().any(|p| words.contains(p))) {
            add(Sector::Libraries, quoted(w), &mut r);
        }
        let museum_compound = words
            .iter()
            .find(|w| {
                ["museum", "museet"]
                    .iter()
                    .any(|s| w.ends_with(s) && w.len() > s.len())
            })
            .copied();
        if let Some(w) = find(&plain, MUSEUMS).or(museum_compound) {
            add(Sector::Museums, quoted(w), &mut r);
        }
        if let Some(w) = find(&plain, HEALTHCARE) {
            add(Sector::Healthcare, quoted(w), &mut r);
        }
        if academia.is_none() {
            if let Some(w) = find(&plain, GOVERNMENT) {
                add(Sector::Government, quoted(w), &mut r);
            }
        }
        if let Some(w) = find(&text, NONPROFIT).or_else(|| find(&plain, NONPROFIT)) {
            if academia.is_none() {
                add(Sector::Nonprofit, quoted(w), &mut r);
            }
        }
    }
    if is_government_domain(host) {
        add(
            Sector::Government,
            format!("the organization's domain {host}"),
            &mut r,
        );
    }
    if let Some(w) = find(&plain, K12_WORDS) {
        add(Sector::K12, quoted(w), &mut r);
    } else if host.contains(".k12.") {
        add(
            Sector::K12,
            format!("the organization's domain {host}"),
            &mut r,
        );
    }
    let whole = plain.trim();
    if let Some(w) = find(&plain, NREN)
        .or_else(|| NREN_NAMES.contains(&whole).then_some(whole))
        .or_else(|| find(&plain, &["géant", "geant"]))
    {
        add(Sector::Nren, quoted(w), &mut r);
    } else if !host.is_empty() && federation_hosts.contains(host) {
        add(
            Sector::Nren,
            format!("the organization's domain {host} is a federation's"),
            &mut r,
        );
    }
    // A public body or a network is not also a nonprofit for being a foundation.
    if r.sectors
        .iter()
        .any(|(s, _)| matches!(s, Sector::Government | Sector::Nren))
    {
        r.sectors.retain(|(s, _)| *s != Sector::Nonprofit);
    }
    let public = r.sectors.iter().any(|(s, _)| {
        matches!(
            s,
            Sector::Academia | Sector::Government | Sector::Nren | Sector::Nonprofit
        )
    });
    if let Some(w) = company.filter(|_| !public) {
        add(Sector::Commercial, quoted(w), &mut r);
    }
    if r.sectors.is_empty() {
        if academic_domain {
            r.hints.push(format!(
                "an academic domain ({host}): a university, or another organization in higher education"
            ));
        }
        if let Some(w) = find(&plain, MAYBE_COMPANY) {
            r.hints.push(format!(
                "\"{w}\" in the name: a company, or a nonprofit with that legal form"
            ));
        }
    }
    r
}

/// `.ac.uk`, `.edu.au`, `.ac.jp`: second-level academic domains under a country code.
fn is_academic_domain(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    let n = labels.len();
    n >= 3 && labels[n - 1].len() == 2 && matches!(labels[n - 2], "ac" | "edu")
}

/// `.gov`, `.mil`, `.gov.uk`, `.gouv.fr`, `.gob.es`, `.govt.nz`, `.gv.at`, `.admin.ch`.
fn is_government_domain(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    let n = labels.len();
    (n >= 2 && matches!(labels[n - 1], "gov" | "mil"))
        || (n >= 3
            && labels[n - 1].len() == 2
            && matches!(labels[n - 2], "gov" | "gouv" | "gob" | "govt" | "gv"))
        || host.ends_with(".admin.ch")
}

/// Curated, then the rules, then Claude's reading, then nothing.
pub fn classify(s: &Service, federation_hosts: &HashSet<String>) -> SectorReading {
    let none = SectorReading {
        sectors: Vec::new(),
        source: None,
        reason: None,
        model: None,
        generated_at: None,
    };
    if let Some(t) = s.curated.as_ref().and_then(|c| c.provider_type.as_deref()) {
        let sectors = from_curated(t);
        if !sectors.is_empty() {
            return SectorReading {
                sectors,
                source: Some("curated".into()),
                reason: Some(format!("Curated provider type \"{t}\".")),
                ..none
            };
        }
    }
    let r = rules(
        provider_name(s).unwrap_or(""),
        organization_host(s).as_deref(),
        federation_hosts,
    );
    if !r.sectors.is_empty() {
        let mut sectors: Vec<Sector> = r.sectors.iter().map(|(s, _)| *s).collect();
        sectors.sort();
        let why: Vec<String> = r.sectors.into_iter().map(|(_, w)| w).collect();
        return SectorReading {
            sectors,
            source: Some("metadata".into()),
            reason: Some(format!("From {}.", why.join("; "))),
            ..none
        };
    }
    if let Some(l) = &s.sector {
        let mut sectors: Vec<Sector> = l
            .sectors
            .iter()
            .filter_map(|k| Sector::from_key(k))
            .collect();
        sectors.sort();
        sectors.dedup();
        if !sectors.is_empty() {
            return SectorReading {
                sectors,
                source: Some("generated".into()),
                reason: Some(l.reason.clone()).filter(|r| !r.is_empty()),
                model: Some(l.model.clone()),
                generated_at: Some(l.generated_at.clone()),
            };
        }
    }
    none
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decide(name: &str, host: &str) -> Vec<Sector> {
        let feds: HashSet<String> = ["incommon.org", "swamid.se"].map(String::from).into();
        let mut s: Vec<Sector> = rules(name, Some(host).filter(|h| !h.is_empty()), &feds)
            .sectors
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        s.sort();
        s
    }

    use Sector::*;

    #[test]
    fn names_and_domains_decide() {
        for (name, host, want) in [
            ("Yale University", "yale.edu", vec![Academia]),
            (
                "Albert-Ludwigs-Universität Freiburg",
                "uni-freiburg.de",
                vec![Academia],
            ),
            ("Université de Caen Normandie", "unicaen.fr", vec![Academia]),
            ("Hochschule München", "hm.edu", vec![Academia]),
            (
                "Deggendorf Institute of Technology",
                "th-deg.de",
                vec![Academia],
            ),
            ("Example Fachhochschule", "", vec![Academia]),
            ("Imperial", "imperial.ac.uk", vec![]),
            (
                "University of Nebraska Medical Center",
                "unmc.edu",
                vec![Academia, Healthcare],
            ),
            (
                "Children's Hospital of Philadelphia",
                "chop.edu",
                vec![Healthcare],
            ),
            (
                "Lawrence Berkeley National Laboratory",
                "lbl.gov",
                vec![Research, Government],
            ),
            ("CLARIN ERIC", "clarin.eu", vec![Research]),
            (
                "National Library of Wales",
                "llyfrgell.cymru",
                vec![Libraries],
            ),
            ("Research Library in Olomouc", "vkol.cz", vec![Libraries]),
            ("Center for Research Libraries", "crl.edu", vec![Libraries]),
            (
                "Bayerische Staatsbibliothek",
                "bsb-muenchen.de",
                vec![Libraries],
            ),
            (
                "Österreichische Nationalbibliothek",
                "onb.ac.at",
                vec![Libraries],
            ),
            ("Rijksmuseum", "", vec![Museums]),
            (
                "Museo Nacional del Prado",
                "museodelprado.es",
                vec![Museums],
            ),
            (
                "The National Archives",
                "nationalarchives.gov.uk",
                vec![Museums, Government],
            ),
            (
                "Austrian Federal Ministry of Women, Science and Research",
                "bmfwf.gv.at",
                vec![Government],
            ),
            ("Norfolk County Council", "norfolk.gov.uk", vec![Government]),
            (
                "Bundesamt für Sicherheit in der Informationstechnik",
                "bsi.bund.de",
                vec![Government],
            ),
            ("Springfield Public Schools", "sps.k12.ma.us", vec![K12]),
            (
                "Bloomsbury Publishing Plc",
                "bloomsbury.com",
                vec![Commercial, Publishers],
            ),
            (
                "Walter de Gruyter Verlag",
                "degruyter.com",
                vec![Publishers],
            ),
            (
                "Swedish Academic Identity Federation (SWAMID)",
                "swamid.se",
                vec![Nren],
            ),
            ("InCommon Operations", "incommon.org", vec![Nren]),
            (
                "German National Research and Education Network, DFN",
                "dfn.de",
                vec![Nren],
            ),
            (
                "Fraunhofer-Gesellschaft zur Förderung der angewandten Forschung e. V.",
                "fraunhofer.de",
                vec![Research, Nonprofit],
            ),
            ("Wellcome Foundation", "", vec![Nonprofit]),
            ("National Science Foundation", "nsf.gov", vec![Government]),
            (
                "Metadata Explorer Tool provided by Géant Association",
                "geant.net",
                vec![Nren],
            ),
            ("Internet2", "internet2.edu", vec![Nren]),
            ("Restena Foundation", "restena.lu", vec![Nren]),
            (
                "Deutsches Krebsforschungszentrum - Stiftung des öffentlichen Rechts",
                "dkfz.de",
                vec![Research, Nonprofit],
            ),
            (
                "Hogrefe Publishing GmbH",
                "hogrefe.de",
                vec![Commercial, Publishers],
            ),
            ("Amberscript B.V.", "amberscript.com", vec![Commercial]),
            (
                "Huayun Culture and Technology Co. Ltd",
                "",
                vec![Commercial],
            ),
        ] {
            assert_eq!(decide(name, host), want, "{name} | {host}");
        }
    }

    #[test]
    fn words_that_do_not_decide() {
        for (name, host, want) in [
            // A university press is a publisher.
            (
                "Cambridge University Press",
                "cambridge.org",
                vec![Publishers],
            ),
            (
                "Tsinghua University Press Limited",
                "tup.com.cn",
                vec![Publishers],
            ),
            // Library products and shadow libraries are not libraries.
            ("Ebook Library - EBL", "ebookscorporation.com", vec![]),
            (
                "Central and Eastern European Online Library",
                "ceeol.com",
                vec![],
            ),
            ("Library Genesis", "libgen.is", vec![]),
            ("Online Computer Library Center Inc.", "oclc.org", vec![]),
            // Companies are not museums, hospitals, or agencies for a word in their name.
            ("Munzinger-Archiv GmbH", "munzinger.de", vec![Commercial]),
            ("MA Healthcare Limited", "markallengroup.com", vec![]),
            (
                "Copyright Licensing Agency Limited (The)",
                "cla.co.uk",
                vec![],
            ),
            (
                "Government Solutions LLC",
                "govsolutions.com",
                vec![Commercial],
            ),
            ("British Broadcasting Corporation", "bbc.co.uk", vec![]),
            // A public university is academia, not government.
            (
                "University of California - Office of the President",
                "ucop.edu",
                vec![Academia],
            ),
            (
                "Royal College of Nursing Publishing",
                "rcni.com",
                vec![Publishers],
            ),
            ("UniversityTickets", "universitytickets.com", vec![]),
            (
                "PlutoF Data management and Publishing Platform",
                "plutof.ut.ee",
                vec![],
            ),
            // "Inc." and "Ltd" alone are left to Claude.
            ("Zoom Video Communications, Inc.", "zoom.us", vec![]),
            ("Center for Open Science, Inc.", "cos.io", vec![]),
            ("EDUCAUSE", "educause.edu", vec![]),
            ("Diamond Light Source Ltd", "diamond.ac.uk", vec![]),
            (
                "Department of Physics, University of Oxford",
                "physics.ox.ac.uk",
                vec![Academia],
            ),
        ] {
            assert_eq!(decide(name, host), want, "{name} | {host}");
        }
        let r = rules("Zoom Video Communications, Inc.", None, &HashSet::new());
        assert!(r.hints[0].contains("\"inc\""), "{:?}", r.hints);
    }

    #[test]
    fn curated_types_map_onto_the_vocabulary() {
        assert_eq!(from_curated("commercial"), [Commercial]);
        assert_eq!(from_curated("Non-Commercial"), [Nonprofit]);
        assert_eq!(from_curated("government"), [Government]);
        assert_eq!(
            from_curated("university / hospital"),
            [Academia, Healthcare]
        );
        assert!(from_curated("something else").is_empty());
    }

    #[test]
    fn curated_then_rules_then_claude() {
        use crate::model::{Curated, SectorLabel};
        let mut s = Service::default();
        s.organization
            .display_names
            .insert("en".into(), "Example Library Software GmbH".into());
        let feds = HashSet::new();
        let r = classify(&s, &feds);
        assert_eq!(
            (r.sectors.clone(), r.source.as_deref()),
            (vec![Commercial], Some("metadata"))
        );
        assert!(r.reason.unwrap().contains("gmbh"));

        s.curated = Some(Curated {
            provider_type: Some("nonprofit".into()),
            ..Default::default()
        });
        assert_eq!(classify(&s, &feds).source.as_deref(), Some("curated"));

        let mut t = Service::default();
        t.organization
            .display_names
            .insert("en".into(), "Zoom Video Communications, Inc.".into());
        assert_eq!(classify(&t, &feds).source, None);
        t.sector = Some(SectorLabel {
            sectors: vec!["commercial".into(), "made-up".into()],
            reason: "A video conferencing company.".into(),
            model: "m".into(),
            generated_at: "2026-10-29T00:00:00Z".into(),
        });
        let r = classify(&t, &feds);
        assert_eq!(
            (r.sectors, r.source.as_deref()),
            (vec![Commercial], Some("generated"))
        );
    }

    #[test]
    fn keys_and_hosts() {
        for s in Sector::ALL {
            assert_eq!(Sector::from_key(s.key()), Some(s));
        }
        assert_eq!(
            host_of("https://www.Yale.edu/about"),
            Some("yale.edu".into())
        );
        assert_eq!(host_of("not a url"), None);
        let feds = [Federation {
            url: Some("https://www.swamid.se/".into()),
            registration_authority: "http://www.swamid.se/".into(),
            ..Default::default()
        }];
        assert_eq!(federation_hosts(&feds), ["swamid.se".to_owned()].into());
        assert!(is_government_domain("nih.gov") && is_government_domain("x.gouv.fr"));
        assert!(!is_government_domain("gov.example.com"));
    }
}
