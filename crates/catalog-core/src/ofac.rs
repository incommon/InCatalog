// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Services whose organization may be on the US Treasury's sanctions lists, published
//! by its Office of Foreign Assets Control (OFAC).
//!
//! OFAC publishes two lists as files from its Sanctions List Service: the Specially
//! Designated Nationals and Blocked Persons list ([`SDN_URL`]) and the consolidated
//! list of its other, non-SDN lists ([`CONSOLIDATED_URL`]). Both share one XML
//! layout, an `sdnEntry` per person, entity, vessel, or aircraft, and carry the date
//! they were published.
//!
//! Only entries for entities are compared, never individuals, vessels, or
//! aircraft. Each entry's name and strong a.k.a.s are compared with the provider's
//! organization name, and each website it lists with the service's hosts:
//!
//! - **Names** match only when equal after [`normalize_name`]: case, punctuation and
//!   a trailing legal form (`LLC`, `Co., Ltd.`, `GmbH`) are ignored, nothing else.
//!   OFAC's weak a.k.a.s, which it says are too broad to screen on, are not used.
//! - **Domains** match when a host of the service (its entityID, login endpoints,
//!   organization or information page) is the listed website's host or under it.
//!   A website with a path (a social media page) is not a domain and is skipped.
//!
//! A name is not an identity, so the catalog never states that a provider is
//! sanctioned. Every match is a *possible* match, showing the entry it matched,
//! until the curation overlay confirms or dismisses it ([`Decision`]). Nothing is
//! confirmed automatically, and a decision for an entry no longer on a list does
//! nothing.
//!
//! This is a pointer to OFAC's own lists as of their publication date, not
//! sanctions screening or legal advice; [`SEARCH_URL`] is OFAC's own search.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::BufRead;

use anyhow::{Context, Result};
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::curation::provider_name;
use crate::model::{pick, OfacMatch, Service};

pub const SDN_URL: &str =
    "https://sanctionslistservice.ofac.treas.gov/api/PublicationPreview/exports/SDN.XML";
pub const CONSOLIDATED_URL: &str =
    "https://sanctionslistservice.ofac.treas.gov/api/PublicationPreview/exports/CONSOLIDATED.XML";
/// OFAC's own search of its lists, for anyone who needs an answer.
pub const SEARCH_URL: &str = "https://sanctionssearch.ofac.treas.gov/";
/// The region key of the countries under OFAC's country-wide programs.
pub const COUNTRY_GROUPING: &str = "ofac";

/// Which of OFAC's lists an entry is on.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum List {
    /// The Specially Designated Nationals and Blocked Persons list.
    Sdn,
    /// The consolidated list of OFAC's non-SDN lists.
    Consolidated,
}

impl List {
    pub const ALL: [List; 2] = [List::Sdn, List::Consolidated];

    pub fn key(self) -> &'static str {
        match self {
            List::Sdn => "sdn",
            List::Consolidated => "consolidated",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            List::Sdn => "OFAC's SDN list",
            List::Consolidated => "OFAC's consolidated (non-SDN) list",
        }
    }

    pub fn url(self) -> &'static str {
        match self {
            List::Sdn => SDN_URL,
            List::Consolidated => CONSOLIDATED_URL,
        }
    }
}

/// How far a match has been checked.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum MatchStatus {
    /// A name or domain matched a list entry; nobody has checked that it is the same
    /// organization.
    Possible,
    /// The curation overlay confirmed it is the same organization.
    Confirmed,
}

impl MatchStatus {
    pub const ALL: [MatchStatus; 2] = [MatchStatus::Confirmed, MatchStatus::Possible];

    pub fn key(self) -> &'static str {
        match self {
            MatchStatus::Possible => "possible",
            MatchStatus::Confirmed => "confirmed",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MatchStatus::Possible => "Possible OFAC match",
            MatchStatus::Confirmed => "On an OFAC list, confirmed",
        }
    }
}

/// One entity on a list, as far as matching needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    pub uid: String,
    pub name: String,
    /// Strong a.k.a.s only.
    pub akas: Vec<String>,
    pub programs: Vec<String>,
    /// Hosts of the websites it lists, lowercase, without `www.`.
    pub domains: Vec<String>,
}

/// A list as read: its entities, and how much else it held.
#[derive(Debug, Clone, Default)]
pub struct Parsed {
    /// `Publish_Date`, as `YYYY-MM-DD`.
    pub published: Option<String>,
    pub entities: Vec<Entry>,
    /// Individuals, vessels and aircraft, which are never compared.
    pub skipped: usize,
}

/// Stream one list file. Only `Entity` entries are kept.
pub fn parse_list<R: BufRead>(reader: R) -> Result<Parsed> {
    let mut reader = Reader::from_reader(reader);
    let mut out = Parsed::default();
    // Local names of the open elements, and the text of the innermost one.
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut entry = Entry::default();
    let mut kind = String::new();
    // The a.k.a. or ID being read.
    let (mut aka_name, mut aka_strong) = (String::new(), false);
    let (mut id_type, mut id_number) = (String::new(), String::new());
    let mut buf = Vec::new();

    loop {
        let pos = reader.buffer_position();
        let event = reader
            .read_event_into(&mut buf)
            .with_context(|| format!("XML error after byte {pos}"))?;
        match event {
            Event::Start(ref e) => {
                path.push(e.local_name().as_ref().to_owned());
                text.clear();
            }
            Event::Text(ref t) => text.push_str(&t.xml10_content()),
            Event::GeneralRef(ref r) => {
                let name: &str = r.as_ref();
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    text.push(c);
                } else if let Some(s) = resolve_predefined_entity(name) {
                    text.push_str(s);
                }
            }
            Event::CData(ref t) => text.push_str(t.as_ref()),
            Event::End(_) => {
                let value = text.trim().to_owned();
                let local = path.pop().unwrap_or_default();
                let parent = path.last().map(String::as_str).unwrap_or("");
                match (parent, local.as_str()) {
                    ("publshInformation", "Publish_Date") => out.published = us_date(&value),
                    ("sdnEntry", "uid") => entry.uid = value,
                    ("sdnEntry", "lastName") => entry.name = value,
                    ("sdnEntry", "sdnType") => kind = value,
                    ("programList", "program") => entry.programs.push(value),
                    ("aka", "lastName") => aka_name = value,
                    ("aka", "category") => aka_strong = value.eq_ignore_ascii_case("strong"),
                    ("akaList", "aka") => {
                        if aka_strong && !aka_name.is_empty() {
                            entry.akas.push(std::mem::take(&mut aka_name));
                        }
                        (aka_name, aka_strong) = (String::new(), false);
                    }
                    ("id", "idType") => id_type = value,
                    ("id", "idNumber") => id_number = value,
                    ("idList", "id") => {
                        if id_type == "Website" {
                            if let Some(d) = website_domain(&id_number) {
                                if !entry.domains.contains(&d) {
                                    entry.domains.push(d);
                                }
                            }
                        }
                        (id_type, id_number) = (String::new(), String::new());
                    }
                    (_, "sdnEntry") => {
                        let done = std::mem::take(&mut entry);
                        if kind == "Entity" {
                            out.entities.push(done);
                        } else {
                            out.skipped += 1;
                        }
                        kind.clear();
                    }
                    _ => {}
                }
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

/// `10/28/2026` → `2026-10-28`.
fn us_date(s: &str) -> Option<String> {
    chrono::NaiveDate::parse_from_str(s, "%m/%d/%Y")
        .ok()
        .map(|d| d.format("%Y-%m-%d").to_string())
}

/// Legal forms ignored at the end of a name, after punctuation is gone: `CO., LTD.`
/// is `CO LTD`, and both words go.
const LEGAL_FORMS: &[&str] = &[
    "AB",
    "AG",
    "AO",
    "AS",
    "BHD",
    "BV",
    "CJSC",
    "CO",
    "COMPANY",
    "CORP",
    "CORPORATION",
    "FZCO",
    "FZE",
    "GMBH",
    "INC",
    "INCORPORATED",
    "JSC",
    "KG",
    "LIMITED",
    "LLC",
    "LLP",
    "LTD",
    "NV",
    "OJSC",
    "OOO",
    "OY",
    "PJSC",
    "PLC",
    "PTE",
    "PTY",
    "SA",
    "SAS",
    "SDN",
    "SPA",
    "SRL",
    "ZAO",
];

/// A name as names are compared: uppercase words of letters and digits, without a
/// leading "The" or trailing legal forms. Empty when that leaves too little to
/// compare (one word of under five characters), so short a.k.a.s like `BNC` never
/// match on their own.
pub fn normalize_name(name: &str) -> String {
    let upper = name.to_uppercase();
    let mut words: Vec<&str> = upper
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    while words.last().is_some_and(|w| LEGAL_FORMS.contains(w)) {
        words.pop();
    }
    if words.first() == Some(&"THE") {
        words.remove(0);
    }
    match words.as_slice() {
        [] => String::new(),
        [one] if one.chars().count() < 5 => String::new(),
        _ => words.join(" "),
    }
}

/// The host of a listed website, lowercase and without `www.`, when it names a whole
/// site. `www.bank.ir` and `https://bank.ir/` do; `instagram.com/bank` does not.
pub fn website_domain(site: &str) -> Option<String> {
    let site = site.trim();
    let with_scheme = if site.contains("://") {
        site.to_owned()
    } else {
        format!("https://{site}")
    };
    let url = url_parts(&with_scheme)?;
    let (host, rest) = url;
    if !rest.trim_matches('/').is_empty() {
        return None;
    }
    let host = host.strip_prefix("www.").unwrap_or(&host).to_owned();
    (host.contains('.') && !host.starts_with('.') && !host.ends_with('.')).then_some(host)
}

/// The lowercase host of an `http(s)` URL and everything after it.
fn url_parts(u: &str) -> Option<(String, String)> {
    let (scheme, rest) = u.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let host = authority.rsplit('@').next()?.split(':').next()?;
    let host = host.trim_end_matches('.').to_lowercase();
    (!host.is_empty()).then(|| (host, rest[end..].to_owned()))
}

/// Every host a service answers on or points to: its entityID's, its login
/// endpoints', and its organization and information pages'.
fn service_hosts(s: &Service) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = s.endpoint_hosts.iter().map(|h| h.to_lowercase()).collect();
    for u in std::iter::once(&s.entity_id)
        .chain(s.organization.urls.values())
        .chain(s.information_urls.values())
    {
        if let Some((h, _)) = url_parts(u) {
            out.insert(h);
        }
    }
    out
}

/// Every list, indexed by normalized name and by domain.
#[derive(Debug, Default)]
pub struct Index {
    entries: Vec<(List, Entry)>,
    published: BTreeMap<List, String>,
    by_name: HashMap<String, Vec<usize>>,
    by_domain: HashMap<String, Vec<usize>>,
}

impl Index {
    pub fn add(&mut self, list: List, parsed: Parsed) {
        if let Some(p) = parsed.published {
            self.published.insert(list, p);
        }
        for e in parsed.entities {
            let i = self.entries.len();
            let mut names: Vec<String> = std::iter::once(&e.name)
                .chain(&e.akas)
                .map(|n| normalize_name(n))
                .filter(|n| !n.is_empty())
                .collect();
            names.sort_unstable();
            names.dedup();
            for n in names {
                self.by_name.entry(n).or_default().push(i);
            }
            for d in &e.domains {
                self.by_domain.entry(d.clone()).or_default().push(i);
            }
            self.entries.push((list, e));
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every entry `s` may be tied to, possible until a decision says otherwise.
    pub fn find(&self, s: &Service) -> Vec<OfacMatch> {
        // Entry index → what matched, as (on, ours, theirs).
        let mut hits: BTreeMap<usize, Vec<(String, String, String)>> = BTreeMap::new();
        let names: BTreeSet<&str> = [
            pick(&s.organization.display_names, None),
            pick(&s.organization.names, None),
        ]
        .into_iter()
        .flatten()
        .collect();
        for ours in names {
            let n = normalize_name(ours);
            for &i in self.by_name.get(&n).into_iter().flatten() {
                let e = &self.entries[i].1;
                let theirs = std::iter::once(&e.name)
                    .chain(&e.akas)
                    .find(|x| normalize_name(x) == n)
                    .unwrap_or(&e.name);
                hits.entry(i)
                    .or_default()
                    .push(("name".into(), ours.to_owned(), theirs.clone()));
            }
        }
        for host in service_hosts(s) {
            // `sso.bank.ir` is under `bank.ir`; so is `bank.ir` itself.
            let mut d = host.as_str();
            loop {
                for &i in self.by_domain.get(d).into_iter().flatten() {
                    hits.entry(i)
                        .or_default()
                        .push(("domain".into(), host.clone(), d.to_owned()));
                }
                match d.split_once('.') {
                    Some((_, up)) if up.contains('.') => d = up,
                    _ => break,
                }
            }
        }
        hits.into_iter()
            .map(|(i, on)| {
                let (list, e) = &self.entries[i];
                OfacMatch {
                    status: MatchStatus::Possible,
                    list: *list,
                    uid: e.uid.clone(),
                    name: e.name.clone(),
                    programs: e.programs.clone(),
                    matched_on: on
                        .into_iter()
                        .map(|(on, ours, theirs)| crate::model::MatchBasis { on, ours, theirs })
                        .collect(),
                    list_published: self.published.get(list).cloned(),
                    decided: None,
                    note: None,
                }
            })
            .collect()
    }
}

/// A curator's decision on a possible match, from the curation overlay.
#[derive(Debug, Clone, Deserialize)]
pub struct Decision {
    /// The entry, as `sdn:<uid>` or `consolidated:<uid>`.
    pub entry: String,
    /// The organization display name it applies to (case-insensitive), as for
    /// `providers` in the overlay: every service that organization registers.
    #[serde(default)]
    pub provider: Option<String>,
    /// Or one service, by entityID.
    #[serde(default)]
    pub service: Option<String>,
    pub decision: Verdict,
    /// When it was decided, `YYYY-MM-DD`.
    pub date: String,
    /// Why, for the reader.
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// The service's organization is the listed entity.
    Confirmed,
    /// It is not: a name collision.
    Dismissed,
}

impl Decision {
    fn applies(&self, s: &Service, m: &OfacMatch) -> bool {
        self.entry.eq_ignore_ascii_case(&entry_key(m.list, &m.uid))
            && (self.service.as_deref() == Some(s.entity_id.as_str())
                || self
                    .provider
                    .as_deref()
                    .zip(provider_name(s))
                    .is_some_and(|(a, b)| a.trim().eq_ignore_ascii_case(b.trim())))
    }
}

/// `sdn:36`: how a decision names an entry. UIDs repeat across the two lists.
pub fn entry_key(list: List, uid: &str) -> String {
    format!("{}:{uid}", list.key())
}

/// What [`apply`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    pub possible: usize,
    pub confirmed: usize,
    /// Matches a decision dismissed.
    pub dismissed: usize,
    /// Distinct providers (by organization name) with a match left.
    pub providers: usize,
    /// Decisions that matched nothing: the entry left the list, or the service or
    /// its name changed.
    pub unused_decisions: Vec<String>,
}

/// Attach every match to its service, after the curators' decisions.
pub fn apply(services: &mut [Service], index: &Index, decisions: &[Decision]) -> Applied {
    let mut out = Applied::default();
    let mut used = vec![false; decisions.len()];
    let mut providers = BTreeSet::new();
    for s in services.iter_mut() {
        let mut kept = Vec::new();
        for mut m in index.find(s) {
            match decisions.iter().position(|d| d.applies(s, &m)) {
                Some(d) => {
                    used[d] = true;
                    let d = &decisions[d];
                    match d.decision {
                        Verdict::Dismissed => {
                            out.dismissed += 1;
                            continue;
                        }
                        Verdict::Confirmed => {
                            m.status = MatchStatus::Confirmed;
                            m.decided = Some(d.date.clone());
                            m.note = d.note.clone();
                        }
                    }
                }
                None => m.status = MatchStatus::Possible,
            }
            kept.push(m);
        }
        // Confirmed first, then by list and entry.
        kept.sort_by(|a, b| {
            b.status
                .cmp(&a.status)
                .then(a.list.cmp(&b.list))
                .then(a.uid.cmp(&b.uid))
        });
        match status(&kept) {
            Some(MatchStatus::Confirmed) => out.confirmed += 1,
            Some(MatchStatus::Possible) => out.possible += 1,
            None => {}
        }
        if !kept.is_empty() {
            providers.insert(
                provider_name(s)
                    .unwrap_or(&s.entity_id)
                    .trim()
                    .to_lowercase(),
            );
        }
        s.ofac = kept;
    }
    out.providers = providers.len();
    out.unused_decisions = decisions
        .iter()
        .zip(used)
        .filter(|(_, u)| !u)
        .map(|(d, _)| {
            let who = d
                .service
                .as_deref()
                .or(d.provider.as_deref())
                .unwrap_or("?");
            format!("{} for {who}", d.entry)
        })
        .collect();
    out
}

/// The strongest status among a service's matches: confirmed, else possible.
pub fn status(matches: &[OfacMatch]) -> Option<MatchStatus> {
    matches.iter().map(|m| m.status).max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LangMap;

    const XML: &str = r#"<?xml version="1.0" standalone="yes"?>
<sdnList xmlns="https://sanctionslistservice.ofac.treas.gov/api/PublicationPreview/exports/XML">
  <publshInformation><Publish_Date>10/28/2026</Publish_Date><Record_Count>4</Record_Count></publshInformation>
  <sdnEntry>
    <uid>36</uid><lastName>AEROCARIBBEAN AIRLINES</lastName><sdnType>Entity</sdnType>
    <programList><program>CUBA</program></programList>
    <akaList>
      <aka><uid>12</uid><type>a.k.a.</type><category>strong</category><lastName>AERO-CARIBBEAN</lastName></aka>
      <aka><uid>13</uid><type>a.k.a.</type><category>weak</category><lastName>CARIBBEAN AIR</lastName></aka>
    </akaList>
  </sdnEntry>
  <sdnEntry>
    <uid>306</uid><lastName>BANCO NACIONAL DE CUBA</lastName><sdnType>Entity</sdnType>
    <programList><program>CUBA</program></programList>
    <akaList><aka><uid>1</uid><type>a.k.a.</type><category>strong</category><lastName>BNC</lastName></aka></akaList>
    <idList>
      <id><uid>7</uid><idType>Website</idType><idNumber>www.bnc.cu</idNumber></id>
      <id><uid>8</uid><idType>Website</idType><idNumber>instagram.com/bnc</idNumber></id>
      <id><uid>9</uid><idType>Tax ID No.</idType><idNumber>12345</idNumber></id>
    </idList>
  </sdnEntry>
  <sdnEntry>
    <uid>9640</uid><firstName>Mohammed</firstName><lastName>ACME RESEARCH</lastName><sdnType>Individual</sdnType>
    <programList><program>SDGT</program></programList>
  </sdnEntry>
  <sdnEntry>
    <uid>41</uid><lastName>SEA &amp; SKY</lastName><sdnType>Vessel</sdnType>
  </sdnEntry>
</sdnList>"#;

    const CONSOLIDATED: &str = r#"<sdnList>
  <publshInformation><Publish_Date>10/19/2026</Publish_Date></publshInformation>
  <sdnEntry>
    <uid>30941</uid><lastName>CHINA UNITED NETWORK COMMUNICATIONS GROUP CO., LTD.</lastName><sdnType>Entity</sdnType>
    <programList><program>CMIC-EO13959</program></programList>
  </sdnEntry>
  <sdnEntry>
    <uid>36</uid><lastName>A &amp; B TRADING LLC</lastName><sdnType>Entity</sdnType>
    <programList><program>SDGT</program></programList>
  </sdnEntry>
</sdnList>"#;

    fn index() -> Index {
        let mut ix = Index::default();
        ix.add(List::Sdn, parse_list(XML.as_bytes()).unwrap());
        ix.add(
            List::Consolidated,
            parse_list(CONSOLIDATED.as_bytes()).unwrap(),
        );
        ix
    }

    fn lang(v: &str) -> LangMap {
        [("en".to_owned(), v.to_owned())].into_iter().collect()
    }

    fn svc(entity_id: &str, org: &str) -> Service {
        Service {
            entity_id: entity_id.into(),
            organization: crate::model::Organization {
                display_names: lang(org),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn parses_entities_and_skips_everyone_else() {
        let p = parse_list(XML.as_bytes()).unwrap();
        assert_eq!(p.published.as_deref(), Some("2026-10-28"));
        assert_eq!(p.skipped, 2, "one individual, one vessel");
        assert_eq!(p.entities.len(), 2);
        let bnc = &p.entities[1];
        assert_eq!(bnc.uid, "306");
        assert_eq!(bnc.programs, ["CUBA"]);
        assert_eq!(bnc.akas, ["BNC"]);
        assert_eq!(
            bnc.domains,
            ["bnc.cu"],
            "the Instagram page is not a domain"
        );
        // Weak a.k.a.s are not kept.
        assert_eq!(p.entities[0].akas, ["AERO-CARIBBEAN"]);

        let c = parse_list(CONSOLIDATED.as_bytes()).unwrap();
        assert_eq!(c.published.as_deref(), Some("2026-10-19"));
        assert_eq!(c.entities[1].name, "A & B TRADING LLC");
    }

    #[test]
    fn names_compare_without_case_punctuation_or_legal_form() {
        assert_eq!(
            normalize_name("China United Network Communications Group Co.Ltd"),
            normalize_name("CHINA UNITED NETWORK COMMUNICATIONS GROUP CO., LTD.")
        );
        assert_eq!(
            normalize_name("The Acme Holdings Company, Inc."),
            "ACME HOLDINGS"
        );
        assert_eq!(normalize_name("Universität Wien"), "UNIVERSITÄT WIEN");
        // Too little left to compare.
        assert_eq!(normalize_name("BNC"), "");
        assert_eq!(normalize_name("Co., Ltd."), "");
        assert_eq!(normalize_name("ROSATOM"), "ROSATOM");
    }

    #[test]
    fn websites_are_domains_only_without_a_path() {
        assert_eq!(
            website_domain("www.bankrefah.ir").as_deref(),
            Some("bankrefah.ir")
        );
        assert_eq!(
            website_domain("https://Bank.IR/").as_deref(),
            Some("bank.ir")
        );
        assert_eq!(
            website_domain("http://bank.ir:8080").as_deref(),
            Some("bank.ir")
        );
        assert_eq!(website_domain("t.me/somechannel"), None);
        assert_eq!(website_domain("localhost"), None);
        assert_eq!(website_domain("ftp://bank.ir"), None);
    }

    #[test]
    fn a_name_collision_stays_a_possible_match() {
        let ix = index();
        let s = svc(
            "https://sp-unicom.carsi.edu.cn/shibboleth",
            "China United Network Communications Group Co.Ltd",
        );
        let m = ix.find(&s);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].status, MatchStatus::Possible);
        assert_eq!(
            (m[0].list, m[0].uid.as_str()),
            (List::Consolidated, "30941")
        );
        assert_eq!(m[0].programs, ["CMIC-EO13959"]);
        assert_eq!(m[0].list_published.as_deref(), Some("2026-10-19"));
        assert_eq!(m[0].matched_on[0].on, "name");
        assert_eq!(
            m[0].matched_on[0].theirs,
            "CHINA UNITED NETWORK COMMUNICATIONS GROUP CO., LTD."
        );
    }

    #[test]
    fn strong_akas_match_and_weak_ones_do_not() {
        let ix = index();
        assert_eq!(
            ix.find(&svc("https://a.example/sp", "Aero-Caribbean"))
                .len(),
            1
        );
        assert!(ix
            .find(&svc("https://a.example/sp", "Caribbean Air"))
            .is_empty());
    }

    #[test]
    fn no_individual_is_ever_matched() {
        let ix = index();
        // "ACME RESEARCH" is an individual's entry; "SEA & SKY" is a vessel.
        for org in ["Acme Research", "Mohammed Acme Research", "Sea & Sky"] {
            assert!(
                ix.find(&svc("https://a.example/sp", org)).is_empty(),
                "{org}"
            );
        }
    }

    #[test]
    fn domains_match_the_site_and_hosts_under_it() {
        let ix = index();
        let mut s = svc("https://sso.bnc.cu/shibboleth", "Some Bank");
        let m = ix.find(&s);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].uid, "306");
        assert_eq!(m[0].matched_on[0].on, "domain");
        assert_eq!(m[0].matched_on[0].ours, "sso.bnc.cu");
        assert_eq!(m[0].matched_on[0].theirs, "bnc.cu");
        // Not a host that merely ends in the same letters.
        s.entity_id = "https://notbnc.cu/sp".into();
        assert!(ix.find(&s).is_empty());
        s.entity_id = "urn:mace:bnc.cu".into();
        s.endpoint_hosts = vec!["login.bnc.cu".into()];
        assert_eq!(ix.find(&s).len(), 1);
    }

    #[test]
    fn uids_repeat_across_lists_so_decisions_name_the_list() {
        let ix = index();
        let s = svc("https://a.example/sp", "A & B Trading");
        let m = ix.find(&s);
        assert_eq!(m.len(), 1);
        assert_eq!(entry_key(m[0].list, &m[0].uid), "consolidated:36");
    }

    fn decision(
        entry: &str,
        provider: Option<&str>,
        service: Option<&str>,
        v: Verdict,
    ) -> Decision {
        Decision {
            entry: entry.into(),
            provider: provider.map(Into::into),
            service: service.map(Into::into),
            decision: v,
            date: "2026-11-01".into(),
            note: Some("checked against the registry".into()),
        }
    }

    #[test]
    fn only_curation_confirms_or_dismisses() {
        let ix = index();
        let mut services = vec![
            svc("https://a.bnc.cu/sp", "Banco Nacional de Cuba"),
            svc("https://b.example/sp", "Aerocaribbean Airlines"),
            svc("https://c.example/sp", "Aerocaribbean Airlines Ltd"),
            svc("https://d.example/sp", "Nobody"),
        ];
        // Undecided, everything is possible.
        let a = apply(&mut services, &ix, &[]);
        assert_eq!(
            (a.possible, a.confirmed, a.dismissed, a.providers),
            (3, 0, 0, 3)
        );
        assert_eq!(status(&services[0].ofac), Some(MatchStatus::Possible));
        // Matched on name and domain, it is still one possible match.
        assert_eq!(services[0].ofac.len(), 1);
        assert_eq!(services[0].ofac[0].matched_on.len(), 2);

        let decisions = [
            decision(
                "SDN:306",
                Some("banco nacional de cuba"),
                None,
                Verdict::Confirmed,
            ),
            decision(
                "sdn:36",
                None,
                Some("https://b.example/sp"),
                Verdict::Dismissed,
            ),
            decision("sdn:99999", Some("Gone Ltd"), None, Verdict::Confirmed),
        ];
        let a = apply(&mut services, &ix, &decisions);
        assert_eq!(
            (a.possible, a.confirmed, a.dismissed, a.providers),
            (1, 1, 1, 2)
        );
        let m = &services[0].ofac[0];
        assert_eq!(m.status, MatchStatus::Confirmed);
        assert_eq!(m.decided.as_deref(), Some("2026-11-01"));
        assert!(m.note.is_some());
        assert!(
            services[1].ofac.is_empty(),
            "dismissed for this service only"
        );
        assert_eq!(status(&services[2].ofac), Some(MatchStatus::Possible));
        assert!(services[3].ofac.is_empty());
        assert_eq!(a.unused_decisions, ["sdn:99999 for Gone Ltd"]);
    }

    #[test]
    fn an_entry_off_the_list_confirms_nothing() {
        let mut services = vec![svc("https://a.bnc.cu/sp", "Banco Nacional de Cuba")];
        let decisions = [decision(
            "sdn:306",
            Some("Banco Nacional de Cuba"),
            None,
            Verdict::Confirmed,
        )];
        let a = apply(&mut services, &Index::default(), &decisions);
        assert!(services[0].ofac.is_empty());
        assert_eq!(a.unused_decisions.len(), 1);
    }

    #[test]
    fn malformed_xml_is_an_error() {
        assert!(parse_list(&b"<sdnList><sdnEntry></sdnList>"[..]).is_err());
    }
}
