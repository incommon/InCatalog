// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Whether an entry is a live service or a test, staging, or development instance.
//!
//! The metadata decides first: a marker word in the entityID's host or path, in
//! every endpoint host, or in the service's name (`test.`, `-dev`, `staging`,
//! `sandbox`, `qa`, "(Test)"). The rules are deliberately narrow, because a
//! marker wins over Claude's reading: "Test Prep Academy", `contest.example.org`
//! and "Office of Development" are not test or development instances. When no
//! marker is found, the environment Claude read while writing the summary
//! ([`crate::model::Enrichment::environment`]) is used, and the result records
//! which of the two decided.
//!
//! Metadata never shows that something *is* production, so `production` only ever
//! comes from Claude's reading; an entry with no marker and no summary is `unknown`.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::model::{pick, Service};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    Production,
    Test,
    Staging,
    Development,
    Unknown,
}

impl Environment {
    pub const ALL: [Environment; 5] = [
        Environment::Production,
        Environment::Test,
        Environment::Staging,
        Environment::Development,
        Environment::Unknown,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Environment::Production => "production",
            Environment::Test => "test",
            Environment::Staging => "staging",
            Environment::Development => "development",
            Environment::Unknown => "unknown",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Environment::Production => "Production",
            Environment::Test => "Test",
            Environment::Staging => "Staging",
            Environment::Development => "Development",
            Environment::Unknown => "Unknown",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.key() == k)
    }

    /// Test, staging, and development instances.
    pub fn non_production(self) -> bool {
        matches!(
            self,
            Environment::Test | Environment::Staging | Environment::Development
        )
    }
}

/// An entry's environment and what decided it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct EnvironmentReading {
    pub value: Environment,
    /// `metadata` (a marker in the entityID, the endpoint hosts, or the name) or
    /// `generated` (Claude's reading in the summary); absent when neither said.
    pub source: Option<String>,
    /// The marker that decided it, when the metadata did, e.g. `"test" in the
    /// entityID's host`.
    pub evidence: Option<String>,
}

/// The entityID's or the name's reading, then Claude's, then the endpoints', then
/// `unknown`.
pub fn classify(s: &Service) -> EnvironmentReading {
    let from = |value, source: &str, evidence: Option<String>| EnvironmentReading {
        value,
        source: Some(source.into()),
        evidence,
    };
    if let Some((value, evidence)) = from_metadata(s) {
        return from(value, "metadata", Some(evidence));
    }
    let generated = s
        .enrichment
        .as_ref()
        .and_then(|e| Environment::from_key(&e.environment))
        .filter(|e| *e != Environment::Unknown);
    if let Some(value) = generated {
        return from(value, "generated", None);
    }
    if let Some((value, evidence)) = from_endpoints(s) {
        return from(value, "metadata", Some(evidence));
    }
    EnvironmentReading {
        value: Environment::Unknown,
        source: None,
        evidence: None,
    }
}

/// A marker in the entityID's host, its path, or the name, in that order, with a
/// description of where it was found.
pub fn from_metadata(s: &Service) -> Option<(Environment, String)> {
    let (host, path) = split_entity_id(&s.entity_id);
    if let Some((env, word)) = in_host(host) {
        return Some((env, format!("\"{word}\" in the entityID's host")));
    }
    if let Some((env, word)) = in_path(path) {
        return Some((env, format!("\"{word}\" in the entityID's path")));
    }
    let names = pick(&s.display_names, None)
        .map(|n| (n, "display name"))
        .into_iter()
        .chain(pick(&s.service_names, None).map(|n| (n, "service name")));
    for (name, what) in names {
        if let Some((env, word)) = in_name(name) {
            return Some((env, format!("\"{word}\" in the {what}")));
        }
    }
    None
}

/// A marker in every endpoint host, all agreeing. Weaker than Claude's reading: an
/// SP whose entityID and name look live but whose only endpoint is
/// `wp-test.example.edu` is as often a live service pointed at shared
/// infrastructure as a test registration, so endpoints decide only when there is
/// no summary to go by.
pub fn from_endpoints(s: &Service) -> Option<(Environment, String)> {
    let found: Vec<Option<(Environment, String)>> =
        s.endpoint_hosts.iter().map(|h| in_host(h)).collect();
    let (env, word) = found.first()?.clone()?;
    found
        .iter()
        .all(|f| f.as_ref().is_some_and(|(e, _)| *e == env))
        .then(|| (env, format!("\"{word}\" in every endpoint host")))
}

/// Words that mark an instance wherever they stand as a word of their own.
const MARKERS: &[(&str, Environment)] = &[
    ("test", Environment::Test),
    ("tst", Environment::Test),
    ("qa", Environment::Test),
    ("uat", Environment::Test),
    ("sandbox", Environment::Test),
    ("sbx", Environment::Test),
    ("demo", Environment::Test),
    ("nonprod", Environment::Test),
    ("nonproduction", Environment::Test),
    ("staging", Environment::Staging),
    ("stg", Environment::Staging),
    ("preprod", Environment::Staging),
    ("preproduction", Environment::Staging),
    ("dev", Environment::Development),
    ("devel", Environment::Development),
];

/// Words that mark an instance in a host or path, where nobody writes prose, but
/// in a name only when they stand apart ("(stage)", "— Development") or name the
/// instance ("Development Instance"): "Professional Development" and "Main Stage"
/// are services.
const WEAK_MARKERS: &[(&str, Environment)] = &[
    ("testing", Environment::Test),
    ("stage", Environment::Staging),
    ("development", Environment::Development),
];

/// Words that turn a weak marker before them into an instance's name.
const INSTANCE_NOUNS: &[&str] = &[
    "instance",
    "instances",
    "environment",
    "env",
    "server",
    "site",
    "system",
    "sp",
    "version",
    "copy",
    "tier",
];

/// "test" in a name that is about tests rather than a test instance: the word
/// after it, then the word before it.
const TEST_NEXT: &[&str] = &[
    "prep",
    "preparation",
    "of",
    "center",
    "centre",
    "centers",
    "centres",
    "bank",
    "banks",
    "taking",
    "taker",
    "takers",
    "score",
    "scores",
    "scoring",
    "delivery",
    "series",
    "administration",
    "proctoring",
    "kitchen",
];
const TEST_BEFORE: &[&str] = &[
    "practice",
    "placement",
    "drug",
    "covid",
    "english",
    "language",
    "standardized",
    "aptitude",
    "diagnostic",
    "proficiency",
    "skills",
    "entrance",
    "admission",
    "admissions",
    "adaptive",
    "speed",
    "personality",
    "hearing",
    "vision",
    "blood",
    "a",
];

/// Words that end in "test" without being one.
const NOT_TEST_SUFFIX: &[&str] = &[
    "contest",
    "latest",
    "attest",
    "protest",
    "detest",
    "greatest",
    "smartest",
    "shortest",
    "fastest",
    "brightest",
    "hottest",
    "neatest",
    "sweetest",
    "fittest",
];

/// Words that end in "stage" without being one.
const NOT_STAGE_SUFFIX: &[&str] = &["backstage", "onstage", "offstage", "upstage", "frontstage"];

/// Second-level labels under which the registered name is the third label from the
/// right (`example.ac.uk`), for the country-code domains that use them.
const SECOND_LEVEL: &[&str] = &["ac", "co", "com", "edu", "gov", "org", "net", "or", "ne"];

fn marker(word: &str) -> Option<Environment> {
    MARKERS
        .iter()
        .chain(WEAK_MARKERS)
        .find(|(w, _)| *w == word)
        .map(|(_, e)| *e)
}

/// `(host, path)` of a URL entityID; a URN is all host-like (`urn:mace:test:sp`).
fn split_entity_id(entity_id: &str) -> (&str, &str) {
    match entity_id.split_once("://") {
        Some((_, rest)) => {
            let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
            let host = rest[..end].rsplit('@').next().unwrap_or("");
            let host = host.split(':').next().unwrap_or(host);
            (host, &rest[end..])
        }
        None => ("", entity_id),
    }
}

/// A marker in a host name, below the registered domain: that one is the
/// organization's name (`devry.edu`, `bsi-dev.net`), and the top-level domain
/// (`.dev`) says nothing either. Labels split into words at `-`, `_`, and digits
/// (`conf-test`, `s3test5`), and a label may also run a marker into another word
/// (`logintest`, `testshib`, `osudev`, `shibstg`, `ermspqa`).
fn in_host(host: &str) -> Option<(Environment, String)> {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    let n = labels.len();
    if n < 2 {
        return None;
    }
    let registered = if n >= 3 && labels[n - 1].len() == 2 && SECOND_LEVEL.contains(&labels[n - 2])
    {
        n - 3
    } else {
        n - 2
    };
    for label in &labels[..registered] {
        for part in split_parts(label) {
            if let Some(env) = marker(&part) {
                return Some((env, part));
            }
            if let Some(hit) = run_together(&part) {
                return Some(hit);
            }
        }
    }
    None
}

/// A marker run into another word: `testshib`, `logintest`, `osudev`, `mpclkuostage`.
fn run_together(part: &str) -> Option<(Environment, String)> {
    let longer = |m: &str| part.len() > m.len();
    if part.starts_with("test")
        && longer("test")
        && !part.starts_with("testim")
        && !part.starts_with("testif")
        && !part.starts_with("testament")
    {
        return Some((Environment::Test, "test".into()));
    }
    if part.ends_with("test") && longer("test") && !NOT_TEST_SUFFIX.contains(&part) {
        return Some((Environment::Test, "test".into()));
    }
    if (part.starts_with("staging") || part.ends_with("staging")) && longer("staging") {
        return Some((Environment::Staging, "staging".into()));
    }
    if part.ends_with("stage") && longer("stage") && !NOT_STAGE_SUFFIX.contains(&part) {
        return Some((Environment::Staging, "stage".into()));
    }
    if part.ends_with("stg") && longer("stg") {
        return Some((Environment::Staging, "stg".into()));
    }
    if part.ends_with("dev") && longer("dev") {
        return Some((Environment::Development, "dev".into()));
    }
    for m in ["qa", "uat", "tst"] {
        if part.ends_with(m) && longer(m) && part != "squat" {
            return Some((Environment::Test, m.into()));
        }
    }
    None
}

/// A marker as a word of its own in the path: `/sp/staging/shibboleth`,
/// `/stg-shibboleth`, `/StarRezPortalXDev/`, `/metadata.php/testusnh`.
fn in_path(path: &str) -> Option<(Environment, String)> {
    for segment in path.split(['/', '?', '&', '=', '#', ':']) {
        for word in camel_words(segment) {
            for part in split_parts(&word) {
                if let Some(env) = marker(&part) {
                    return Some((env, part));
                }
                if part.starts_with("test") && part.len() > 4 && !part.starts_with("testim") {
                    return Some((Environment::Test, "test".into()));
                }
            }
        }
    }
    None
}

/// A marker in a display or service name, read as prose (see [`WEAK_MARKERS`]).
fn in_name(name: &str) -> Option<(Environment, String)> {
    for segment in name_segments(name) {
        let words: Vec<String> = segment
            .split_whitespace()
            .map(|w| {
                w.chars()
                    .filter(|c| c.is_alphanumeric())
                    .collect::<String>()
            })
            .filter(|w| !w.is_empty())
            .collect();
        // "UAT2" and "Staging2" are numbered instances of the same thing.
        let lower: Vec<String> = words
            .iter()
            .map(|w| {
                w.to_lowercase()
                    .trim_end_matches(|c: char| c.is_ascii_digit())
                    .to_owned()
            })
            .collect();
        let stands_apart = lower.iter().all(|w| {
            marker(w).is_some()
                || INSTANCE_NOUNS.contains(&w.as_str())
                || ["and", "or", "only"].contains(&w.as_str())
        });
        for (i, w) in lower.iter().enumerate() {
            let next = lower.get(i + 1).map(String::as_str);
            let before = i.checked_sub(1).map(|j| lower[j].as_str());
            if let Some(&(_, env)) = MARKERS.iter().find(|(m, _)| m == w) {
                let about_tests = w == "test"
                    && (next.is_some_and(|n| TEST_NEXT.contains(&n))
                        || before.is_some_and(|b| TEST_BEFORE.contains(&b)));
                // "Digital Sandbox" is a kind of service, not a sandbox of one.
                let a_product = w == "sandbox" && before == Some("digital");
                if !about_tests && !a_product {
                    return Some((env, words[i].clone()));
                }
            }
            if let Some(&(_, env)) = WEAK_MARKERS.iter().find(|(m, _)| m == w) {
                if stands_apart || next.is_some_and(|n| INSTANCE_NOUNS.contains(&n)) {
                    return Some((env, words[i].clone()));
                }
            }
            // An identifier used as a name: "StarRezPortalXTest", but not "TestOut".
            if let Some(last) = camel_words(&words[i])
                .pop()
                .filter(|_| camel_words(&words[i]).len() > 1)
            {
                if let Some(&(_, env)) = MARKERS.iter().find(|(m, _)| *m == last.to_lowercase()) {
                    return Some((env, last));
                }
            }
        }
    }
    None
}

/// A name cut at brackets and separators (" - ", "—", ":", "|", "/"), so "(Test)"
/// and "Action Data — Development" each leave the marker in a segment of its own.
/// Hyphens inside words ("Non-Production", "e-Learning") are kept, then dropped.
fn name_segments(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut out = vec![String::new()];
    for (i, &c) in chars.iter().enumerate() {
        let spaced = |j: Option<usize>| {
            j.and_then(|j| chars.get(j))
                .is_none_or(|c| c.is_whitespace())
        };
        let cut = matches!(
            c,
            '(' | ')' | '[' | ']' | '{' | '}' | '|' | ':' | '/' | '–' | '—'
        ) || (c == '-' && (spaced(i.checked_sub(1)) || spaced(Some(i + 1))));
        if cut {
            out.push(String::new());
        } else if c != '-' {
            out.last_mut().unwrap().push(c);
        }
    }
    out.retain(|s| !s.trim().is_empty());
    out
}

/// Lowercase words of a host label or path segment, split at punctuation and at
/// letter–digit boundaries: `s3test5` → `s`, `test`; `stg-shibboleth` → `stg`, `shibboleth`.
fn split_parts(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut last_digit = None;
    for c in s.chars() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            last_digit = None;
            continue;
        }
        let digit = c.is_ascii_digit();
        if last_digit.is_some_and(|d| d != digit) && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        last_digit = Some(digit);
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.retain(|p| !p.chars().all(|c| c.is_ascii_digit()));
    out
}

/// Split at camelCase boundaries, keeping case: `StarRezPortalXDev` → `Star`, `Rez`,
/// `Portal`, `X`, `Dev`. Text without inner capitals comes back whole.
fn camel_words(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = vec![String::new()];
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 && c.is_uppercase() {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_lower) {
                out.push(String::new());
            }
        }
        out.last_mut().unwrap().push(c);
    }
    out.retain(|w| !w.is_empty());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Enrichment;

    fn svc(entity_id: &str, name: &str) -> Service {
        let mut s = Service {
            entity_id: entity_id.into(),
            ..Default::default()
        };
        if !name.is_empty() {
            s.display_names.insert("en".into(), name.into());
        }
        s
    }

    fn read(entity_id: &str, name: &str) -> Option<Environment> {
        from_metadata(&svc(entity_id, name)).map(|(e, _)| e)
    }

    use Environment::{Development as Dev, Staging, Test};

    #[test]
    fn markers_in_the_entity_id_host() {
        for (id, env) in [
            ("https://conf-test.it.ucla.edu/incommon", Test),
            ("https://s3test5.andrew.cmu.edu/shibboleth-aio", Test),
            ("https://testusnh.atriumcampus.com/sso", Test),
            ("https://logintest.rit.edu/inc/shibboleth", Test),
            ("https://uat2.penn.finance.cayuse.com/shibboleth", Test),
            ("https://qa-sso.knewknovel.com/entry", Test),
            ("https://ermspqa.ucop.edu/paq", Test),
            ("https://accounts.test.osf.io/shibboleth", Test),
            ("http://alumni-tst.northwestern.edu", Test),
            ("https://shib.sbx.oclc.org/oclcsaml-v1", Test),
            ("https://staging.portal.euromonitor.com/athens", Staging),
            ("https://shibstg.webassign.net/shibboleth", Staging),
            ("https://mpclkuostage.huronclick.com/shibboleth", Staging),
            ("https://stage.mahara.qmul.ac.uk/shibboleth", Staging),
            ("https://dev.eid.colostate.edu/shibboleth", Dev),
            ("https://osudev.onecampus.com", Dev),
            (
                "https://login-dev.helmholtz.de/unitygw/saml-sp-metadata",
                Dev,
            ),
        ] {
            assert_eq!(read(id, ""), Some(env), "{id}");
        }
    }

    #[test]
    fn markers_in_the_path_and_urns() {
        for (id, env) in [
            (
                "https://sp.silverchair.com/karger/staging/shibboleth",
                Staging,
            ),
            ("https://journal-mps.stg.rcni.com/stg-shibboleth", Staging),
            ("https://bu.starrezhousing.com/StarRezPortalXTest/", Test),
            ("https://mit.starrezhousing.com/StarRezPortalXDev/", Dev),
            (
                "https://x.atriumcampus.com/sso/module.php/saml/sp/metadata.php/testusnh",
                Test,
            ),
            ("urn:mace:example.edu:sp:test", Test),
        ] {
            assert_eq!(read(id, ""), Some(env), "{id}");
        }
    }

    #[test]
    fn markers_in_the_name() {
        let id = "https://sp.example.edu/shibboleth";
        for (name, env) in [
            ("RIT Login Test", Test),
            ("ITHAKA TEST Service Provider", Test),
            ("spaces qa", Test),
            ("Penn UAT2 Finance Cayuse Webapp", Test),
            ("University of Nottingham - Alma Premium Sandbox", Test),
            ("SP Demo provided by GARR", Test),
            ("PJ2 UCPath Non-Production", Test),
            ("PreProd- EndNote Shibboleth service", Staging),
            ("UO IRB (stage)", Staging),
            ("Passport (Staging) - Euromonitor International", Staging),
            ("Action Data — Development", Dev),
            ("Division of IT NetID Services Development Instance", Dev),
            ("HDR UK Health Data Gateway DEV", Dev),
            ("NORTHWESTERN StarRezPortalXResidentialExperienceDev", Dev),
        ] {
            assert_eq!(read(id, name), Some(env), "{name}");
        }
    }

    #[test]
    fn words_that_are_not_markers() {
        // Hosts: words that end in a marker, the registered name, the top-level domain.
        for id in [
            "https://contest.example.org/shibboleth",
            "https://latest.example.org/sp",
            "https://www.testament.example.org/sp",
            "https://backstage.example.com/sp",
            "https://sso.devry.edu/sp",
            "https://auth.accord.bsi-dev.net",
            "https://zoomdev.us",
            "https://app.example.dev/saml",
            "https://qatar.example.edu/sp",
            "https://sp.example.test/sp",
            "https://devices.example.edu/shibboleth",
            "https://sp.example.edu/saml/latest",
        ] {
            assert_eq!(read(id, ""), None, "{id}");
        }
        // Names that are about tests, development, or stages.
        let id = "https://sp.example.edu/shibboleth";
        for name in [
            "Test Prep Academy",
            "Pearson Test of English",
            "ProctorU Testing",
            "Testing Center Scheduler",
            "Practice Test Bank",
            "Office of Development",
            "Professional Development Portal",
            "Faculty Development",
            "Main Stage Theatre",
            "Life Stage Planning",
            "TestOut LabSim",
            "Contest Manager",
            "Columbia Digital Sandbox",
            "Developer Portal",
        ] {
            assert_eq!(read(id, name), None, "{name}");
        }
    }

    #[test]
    fn evidence_says_where_the_marker_was() {
        let s = svc("https://sp.example.edu/sp", "Canvas (Test)");
        assert_eq!(
            from_metadata(&s),
            Some((Test, "\"Test\" in the display name".into()))
        );
        let mut s = svc("https://sp.example.edu/sp", "");
        s.service_names.insert("en".into(), "Wiki Sandbox".into());
        assert_eq!(
            from_metadata(&s).unwrap().1,
            "\"Sandbox\" in the service name"
        );
    }

    fn enriched(entity_id: &str, env: &str) -> Service {
        let mut s = svc(entity_id, "Canvas");
        s.enrichment = Some(Enrichment {
            environment: env.into(),
            ..Default::default()
        });
        s
    }

    #[test]
    fn metadata_then_claude_then_endpoints() {
        // A marker beats Claude's reading.
        let r = classify(&enriched(
            "https://canvas-test.example.edu/sp",
            "production",
        ));
        assert_eq!(r.value, Test);
        assert_eq!(r.source.as_deref(), Some("metadata"));

        // Without one, Claude decides.
        let r = classify(&enriched("https://canvas.example.edu/sp", "staging"));
        assert_eq!((r.value, r.source.as_deref()), (Staging, Some("generated")));
        assert_eq!(r.evidence, None);
        let r = classify(&enriched("https://canvas.example.edu/sp", "production"));
        assert_eq!(r.value, Environment::Production);

        // Claude's reading beats endpoints; with no reading, endpoints that all
        // agree decide.
        let mut s = enriched("https://canvas.example.edu/sp", "production");
        s.endpoint_hosts = vec!["canvas-dev.example.edu".into()];
        assert_eq!(classify(&s).value, Environment::Production);
        s.enrichment.as_mut().unwrap().environment = "unknown".into();
        let r = classify(&s);
        assert_eq!(r.value, Dev);
        assert_eq!(
            r.evidence.as_deref(),
            Some("\"dev\" in every endpoint host")
        );
        s.endpoint_hosts.push("canvas.example.edu".into());
        assert_eq!(classify(&s).value, Environment::Unknown);
        s.endpoint_hosts = vec!["a-dev.example.edu".into(), "b-test.example.edu".into()];
        assert_eq!(
            classify(&s).source,
            None,
            "endpoints that disagree say nothing"
        );

        // Nothing at all.
        let r = classify(&svc("https://canvas.example.edu/sp", "Canvas"));
        assert_eq!(
            r,
            EnvironmentReading {
                value: Environment::Unknown,
                source: None,
                evidence: None
            }
        );
    }

    #[test]
    fn keys_round_trip() {
        for e in Environment::ALL {
            assert_eq!(Environment::from_key(e.key()), Some(e));
            assert!(!e.label().is_empty());
        }
        assert_eq!(Environment::from_key("prod"), None);
        assert!(Test.non_production() && !Environment::Production.non_production());
        assert!(!Environment::Unknown.non_production());
    }

    #[test]
    fn splitting() {
        assert_eq!(split_parts("s3test5"), ["s", "test"]);
        assert_eq!(split_parts("stg-shibboleth"), ["stg", "shibboleth"]);
        assert_eq!(
            camel_words("StarRezPortalXDev"),
            ["Star", "Rez", "Portal", "X", "Dev"]
        );
        assert_eq!(camel_words("QA"), ["QA"]);
        assert_eq!(
            split_entity_id("https://u@h.example:8443/p?q"),
            ("h.example", "/p?q")
        );
        assert_eq!(
            name_segments("Staging / Test - Canvas (UAT) e-Learning"),
            ["Staging ", " Test ", " Canvas ", "UAT", " eLearning"]
        );
    }
}
