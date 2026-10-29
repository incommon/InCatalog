// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Release notes: the repository's `CHANGELOG.md`, embedded when the API is built so
//! the notes served always match the running build.
//!
//! The changelog follows Keep a Changelog: one `## [version] - YYYY-MM-DD` section per
//! release, newest first, under an optional `## [Unreleased]`. Each section's body is
//! served as Markdown; the web front end renders a small subset of it.

use std::sync::OnceLock;

use serde::Serialize;
use utoipa::ToSchema;

/// The running version: the workspace version in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

/// What `## [Unreleased]` is served as.
pub const UNRELEASED: &str = "Unreleased";

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct Release {
    /// A SemVer version, or `Unreleased` for changes merged since the last release.
    #[schema(example = "0.1.0")]
    pub version: String,
    /// The release date (YYYY-MM-DD); absent while unreleased.
    pub date: Option<String>,
    /// Whether this is the version the API is running.
    pub current: bool,
    /// The notes as Markdown: `###` headings (Added, Changed, Fixed, …), `-` items,
    /// `` `code` ``, `**bold**` and `[links](url)`.
    pub notes: String,
}

/// Every release in the embedded changelog, newest first.
pub fn all() -> &'static [Release] {
    static RELEASES: OnceLock<Vec<Release>> = OnceLock::new();
    // The tests below parse the same file, so a malformed changelog fails the build's
    // tests rather than a request.
    RELEASES.get_or_init(|| parse(CHANGELOG, VERSION).expect("CHANGELOG.md parses"))
}

/// One release by version, with or without a leading `v`; `unreleased` in any case.
pub fn find(version: &str) -> Option<&'static Release> {
    let v = version.strip_prefix('v').unwrap_or(version);
    all().iter().find(|r| r.version.eq_ignore_ascii_case(v))
}

/// Read a Keep a Changelog file. `current` is the running version, which marks its
/// release; an empty Unreleased section is left out. Fails on a heading that is not a
/// version (or `Unreleased`), a bad date, a release without one, or a version listed
/// twice.
pub fn parse(markdown: &str, current: &str) -> Result<Vec<Release>, String> {
    let mut releases: Vec<Release> = Vec::new();
    let mut body: Vec<&str> = Vec::new();
    for line in markdown.lines() {
        let Some(heading) = line.strip_prefix("## ") else {
            // Link reference definitions (`[0.1.0]: https://…`) close the file.
            if !releases.is_empty() && !is_link_definition(line) {
                body.push(line);
            }
            continue;
        };
        close(&mut releases, &mut body);
        let release = heading_release(heading.trim(), current)?;
        if releases.iter().any(|r| r.version == release.version) {
            return Err(format!("{} is listed twice", release.version));
        }
        releases.push(release);
    }
    close(&mut releases, &mut body);
    // Right after a release, Unreleased is an empty heading: nothing to show.
    releases.retain(|r| r.version != UNRELEASED || !r.notes.is_empty());
    Ok(releases)
}

/// The lines since the last heading are that release's notes.
fn close(releases: &mut [Release], body: &mut Vec<&str>) {
    if let Some(last) = releases.last_mut() {
        last.notes = body.join("\n").trim_matches('\n').trim_end().to_owned();
    }
    body.clear();
}

fn heading_release(heading: &str, current: &str) -> Result<Release, String> {
    let (version, rest) = match heading.strip_prefix('[') {
        Some(h) => h
            .split_once(']')
            .ok_or_else(|| format!("unclosed [ in \"## {heading}\""))?,
        None => heading.split_once(' ').unwrap_or((heading, "")),
    };
    let rest = rest.trim();
    let date = match rest.strip_prefix('-').map(str::trim) {
        Some(d) if is_date(d) => Some(d.to_owned()),
        None if rest.is_empty() => None,
        _ => {
            return Err(format!(
                "\"{rest}\" in \"## {heading}\" is not \"- YYYY-MM-DD\""
            ))
        }
    };
    if version.eq_ignore_ascii_case(UNRELEASED) {
        if date.is_some() {
            return Err("the Unreleased section has a date".into());
        }
        return Ok(Release {
            version: UNRELEASED.into(),
            date,
            current: false,
            notes: String::new(),
        });
    }
    if semver(version).is_none() {
        return Err(format!("\"{version}\" is not a SemVer version"));
    }
    if date.is_none() {
        return Err(format!("release {version} has no date"));
    }
    Ok(Release {
        version: version.into(),
        date,
        current: version == current,
        notes: String::new(),
    })
}

fn is_link_definition(line: &str) -> bool {
    line.starts_with('[') && line.contains("]: ")
}

/// `YYYY-MM-DD`, with a month and day in range.
fn is_date(s: &str) -> bool {
    let num = |r: std::ops::Range<usize>| {
        s.get(r)
            .filter(|p| p.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|p| p.parse::<u32>().ok())
    };
    let sep = s.len() == 10 && s.as_bytes()[4] == b'-' && s.as_bytes()[7] == b'-';
    sep && num(0..4).is_some()
        && num(5..7).is_some_and(|m| (1..=12).contains(&m))
        && num(8..10).is_some_and(|d| (1..=31).contains(&d))
}

/// A SemVer 2.0.0 version's precedence key: the core numbers, then whether it is a
/// full release (a pre-release sorts below it), then the pre-release identifiers.
/// Build metadata is ignored, as SemVer §10 says.
pub fn semver(v: &str) -> Option<(u64, u64, u64, bool, Vec<Ident>)> {
    let (v, build) = v.split_once('+').map_or((v, None), |(v, b)| (v, Some(b)));
    if build.is_some_and(|b| !b.split('.').all(is_ident)) {
        return None;
    }
    let (core, pre) = match v.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (v, None),
    };
    let nums: Vec<u64> = core.split('.').map(numeric).collect::<Option<Vec<u64>>>()?;
    let [major, minor, patch] = nums[..] else {
        return None;
    };
    let pre = match pre {
        None => Vec::new(),
        Some(p) => p
            .split('.')
            .map(|id| match numeric(id) {
                Some(n) => Some(Ident::Num(n)),
                None if is_ident(id) && id.bytes().any(|b| !b.is_ascii_digit()) => {
                    Some(Ident::Alpha(id.to_owned()))
                }
                None => None,
            })
            .collect::<Option<Vec<Ident>>>()?,
    };
    Some((major, minor, patch, pre.is_empty(), pre))
}

/// A pre-release identifier. Numeric ones sort below alphanumeric ones (SemVer §11).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ident {
    Num(u64),
    Alpha(String),
}

fn is_ident(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// A number without leading zeros.
fn numeric(s: &str) -> Option<u64> {
    let ok = !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    (ok && (s == "0" || !s.starts_with('0')))
        .then(|| s.parse().ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Changelog

Preamble that is not a release.

## [Unreleased]

### Added

- Something new, with `code`
  and a second line.

## [1.0.0] - 2026-11-02

### Changed

- **Breaking:** a change.

## [1.0.0-rc.1] - 2026-11-01

## 0.9.0 - 2026-10-01

Plain heading, no brackets.

[Unreleased]: https://example.org/compare/v1.0.0...develop
[1.0.0]: https://example.org/tree/v1.0.0
";

    #[test]
    fn parses_sections_newest_first() {
        let r = parse(SAMPLE, "1.0.0").unwrap();
        let versions: Vec<&str> = r.iter().map(|r| r.version.as_str()).collect();
        assert_eq!(versions, ["Unreleased", "1.0.0", "1.0.0-rc.1", "0.9.0"]);
        assert_eq!(r[0].date, None);
        assert_eq!(
            r[0].notes,
            "### Added\n\n- Something new, with `code`\n  and a second line."
        );
        assert!(!r[0].current && r[1].current && !r[2].current);
        assert_eq!(r[1].date.as_deref(), Some("2026-11-02"));
        assert_eq!(r[2].notes, "", "an empty section is allowed");
        assert_eq!(
            r[3].notes, "Plain heading, no brackets.",
            "link definitions are not notes"
        );
    }

    #[test]
    fn rejects_malformed_headings() {
        for (bad, why) in [
            ("## [1.0] - 2026-01-01", "not SemVer"),
            ("## [01.0.0] - 2026-01-01", "not SemVer"),
            ("## [1.0.0]", "no date"),
            ("## [1.0.0] - 2026-13-01", "bad date"),
            ("## [1.0.0] - 2026-1-1", "bad date"),
            ("## [1.0.0] 2026-01-01", "no dash"),
            ("## [1.0.0 - 2026-01-01", "unclosed"),
            ("## [Unreleased] - 2026-01-01", "dated Unreleased"),
            ("## [1.0.0] - 2026-01-01\n## [1.0.0] - 2026-01-02", "twice"),
        ] {
            assert!(parse(bad, "1.0.0").is_err(), "{why}: {bad}");
        }
        assert_eq!(parse("# Only a title\n\ntext", "1.0.0").unwrap(), []);
        let fresh = parse("## [Unreleased]\n\n## [1.0.0] - 2026-01-01\n\n- x", "1.0.0").unwrap();
        assert_eq!(fresh.len(), 1, "an empty Unreleased is left out");
        assert_eq!(fresh[0].version, "1.0.0");
    }

    #[test]
    fn semver_precedence() {
        // The example ordering in SemVer §11, lowest first.
        let ordered = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1+build.5",
            "1.1.0",
            "2.0.0",
        ];
        let keys: Vec<_> = ordered.iter().map(|v| semver(v).unwrap()).collect();
        assert!(keys.windows(2).all(|w| w[0] < w[1]), "{ordered:?}");
        for bad in [
            "",
            "1",
            "1.0",
            "1.0.0.0",
            "v1.0.0",
            "1.0.0-",
            "1.0.0-01",
            "1.0.0-a..b",
            "1.0.0+",
            "1.0.0+a_b",
            "1.x.0",
        ] {
            assert!(semver(bad).is_none(), "{bad}");
        }
    }

    /// The real changelog: it parses, its releases run newest first, and the running
    /// version has notes, so a release cannot ship without them.
    #[test]
    fn the_changelog_describes_this_build() {
        let releases = all();
        let dated: Vec<_> = releases
            .iter()
            .filter(|r| r.version != UNRELEASED)
            .collect();
        assert!(
            releases.iter().skip(1).all(|r| r.version != UNRELEASED),
            "Unreleased comes first"
        );
        let keys: Vec<_> = dated.iter().map(|r| semver(&r.version).unwrap()).collect();
        assert!(keys.windows(2).all(|w| w[0] > w[1]), "newest first");
        assert!(dated.windows(2).all(|w| w[0].date >= w[1].date));
        let current = find(VERSION).unwrap_or_else(|| {
            panic!("CHANGELOG.md has no section for {VERSION}, the Cargo.toml version")
        });
        assert!(current.current && !current.notes.is_empty());
        assert_eq!(releases.iter().filter(|r| r.current).count(), 1);
        assert_eq!(find(&format!("v{VERSION}")), Some(current));
        assert!(find("9.9.9").is_none());
    }
}
