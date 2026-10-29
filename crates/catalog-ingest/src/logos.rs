// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Logos: use the metadata's where it works, find one on the website where it doesn't.
//!
//! 1. Every `https:` `mdui:Logo` URL is fetched. One that answers with an error page,
//!    a 404, or a dead host is marked `broken` (about 3 in 10 in a sample of the live
//!    aggregate) and never shown. Inconclusive answers (403, 429, timeouts) are left
//!    alone: a browser may still load them.
//! 2. A service left with no working logo gets one from its organization's website.
//!    Ingest fetches the homepage itself and lists the candidate images (icon links,
//!    `og:image`, schema.org `logo`, `<img>` marked as a logo). Claude picks the
//!    organization's logo from that numbered list, and ingest confirms the pick is a
//!    real image of a usable size. Claude never browses: the worst a hostile page can
//!    do is make it pick the wrong candidate, which the image check still screens.
//!
//! Every URL here comes from metadata written by thousands of registrants, so the
//! HTTP client refuses private, loopback, link-local, and cloud-metadata addresses,
//! including after redirects and DNS resolution.
//!
//! Results are cached (`logo-cache.json` beside the snapshot) so each URL and site is
//! looked up once and rechecked on a schedule, not on every six-hourly ingest.

use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Result};
use catalog_claude::Claude;
use catalog_core::model::{DiscoveredLogo, Service};
use catalog_store::Location;
use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (compatible; InCatalogLogoCheck/",
    env!("CARGO_PKG_VERSION"),
    ")"
);
const MAX_PAGE_BYTES: usize = 2 << 20;
const MAX_IMAGE_BYTES: usize = 1 << 20;
const MAX_CANDIDATES: usize = 25;
/// Smaller than this (when the format declares a size) looks blurry in a 40px tile.
const MIN_PIXELS: u32 = 32;

pub struct Options {
    pub cache: Location,
    /// Most metadata logo URLs to check per run (unchecked ones wait for the next run).
    pub max_checks: usize,
    /// Most websites to look up per run.
    pub max_lookups: usize,
    /// `None` when no Anthropic key is configured: checks still run, lookups do not.
    pub claude: Option<Claude>,
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub metadata_logos_checked: usize,
    pub metadata_logos_broken: usize,
    pub services_without_working_logo: usize,
    pub sites_looked_up: usize,
    pub sites_waiting: usize,
    pub services_given_website_logo: usize,
    pub lookup_skipped_reason: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    #[serde(default)]
    checks: BTreeMap<String, Check>,
    #[serde(default)]
    sites: BTreeMap<String, Site>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Check {
    /// `ok`, `broken`, or `unknown`.
    status: String,
    detail: String,
    checked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Site {
    logo: Option<String>,
    page: String,
    chosen_by: Option<String>,
    /// `found`, `none`, or `error`.
    outcome: String,
    detail: String,
    checked_at: DateTime<Utc>,
}

impl Check {
    fn fresh(&self, now: DateTime<Utc>) -> bool {
        let days = match self.status.as_str() {
            "ok" => 14,
            "broken" => 7,
            _ => 3,
        };
        now - self.checked_at < chrono::Duration::days(days)
    }
}

impl Site {
    fn fresh(&self, now: DateTime<Utc>) -> bool {
        let days = match self.outcome.as_str() {
            "found" => 30,
            "none" => 14,
            _ => 3,
        };
        now - self.checked_at < chrono::Duration::days(days)
    }
}

pub async fn enrich(services: &mut [Service], opts: Options) -> Result<Report> {
    let now = Utc::now();
    let mut cache: Cache = match opts.cache.read(None).await {
        Ok(Some(blob)) => serde_json::from_slice(&blob.bytes).unwrap_or_default(),
        _ => Cache::default(),
    };
    let http = guarded_client()?;
    let mut report = Report::default();

    // 1. Check metadata logos.
    let mut wanted: Vec<String> = Vec::new();
    for s in services.iter() {
        for l in &s.logos {
            if l.url.starts_with("https://") && !wanted.contains(&l.url) {
                wanted.push(l.url.clone());
            }
        }
    }
    let to_check: Vec<String> = wanted
        .into_iter()
        .filter(|u| !cache.checks.get(u).is_some_and(|c| c.fresh(now)))
        .take(opts.max_checks)
        .collect();
    report.metadata_logos_checked = to_check.len();
    let checked: Vec<(String, Check)> = stream::iter(to_check)
        .map(|url| {
            let http = http.clone();
            async move {
                let (status, detail) = match check_image(&http, &url, false).await {
                    ImageCheck::Ok => ("ok", String::new()),
                    ImageCheck::Broken(d) => ("broken", d),
                    ImageCheck::Unknown(d) => ("unknown", d),
                };
                let check = Check {
                    status: status.into(),
                    detail,
                    checked_at: Utc::now(),
                };
                (url, check)
            }
        })
        .buffer_unordered(24)
        .collect()
        .await;
    cache.checks.extend(checked);
    for s in services.iter_mut() {
        for l in &mut s.logos {
            l.broken = cache
                .checks
                .get(&l.url)
                .is_some_and(|c| c.status == "broken");
        }
    }
    report.metadata_logos_broken = cache
        .checks
        .values()
        .filter(|c| c.status == "broken")
        .count();

    // 2. Find website logos for services left without a working one.
    let mut by_site: HashMap<String, (String, String, Vec<usize>)> = HashMap::new();
    for (i, s) in services.iter().enumerate() {
        if catalog_core::search::best_logo(s).is_some() {
            continue;
        }
        report.services_without_working_logo += 1;
        let Some((origin, page)) = site_of(s) else {
            continue;
        };
        let org = catalog_core::curation::provider_name(s)
            .unwrap_or(&s.entity_id)
            .to_owned();
        by_site
            .entry(origin)
            .or_insert_with(|| (page, org, Vec::new()))
            .2
            .push(i);
    }
    let mut stale: Vec<(String, String, String, usize)> = by_site
        .iter()
        .filter(|(origin, _)| !cache.sites.get(*origin).is_some_and(|s| s.fresh(now)))
        .map(|(origin, (page, org, ids))| (origin.clone(), page.clone(), org.clone(), ids.len()))
        .collect();
    // Sites behind the most services first: one lookup fixes the most tiles.
    stale.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.0.cmp(&b.0)));
    match &opts.claude {
        None => {
            report.sites_waiting = stale.len();
            if !stale.is_empty() {
                report.lookup_skipped_reason = Some(
                    "no Anthropic key configured (ANTHROPIC_API_KEY or ANTHROPIC_API_KEY_SSM_PARAM)".into(),
                );
            }
        }
        Some(claude) => {
            report.sites_waiting = stale.len().saturating_sub(opts.max_lookups);
            stale.truncate(opts.max_lookups);
            report.sites_looked_up = stale.len();
            let claude = Arc::new(claude.clone());
            let found: Vec<(String, Site)> = stream::iter(stale)
                .map(|(origin, page, org, _)| {
                    let (http, claude) = (http.clone(), claude.clone());
                    async move {
                        let site = lookup(&http, &claude, &page, &org).await;
                        tracing::info!(%origin, outcome = %site.outcome, logo = ?site.logo, detail = %site.detail, "website logo");
                        (origin, site)
                    }
                })
                .buffer_unordered(6)
                .collect()
                .await;
            cache.sites.extend(found);
        }
    }
    for (origin, (_, _, ids)) in &by_site {
        let Some(site) = cache.sites.get(origin) else {
            continue;
        };
        let Some(logo) = &site.logo else { continue };
        for &i in ids {
            services[i].discovered_logo = Some(DiscoveredLogo {
                url: logo.clone(),
                page: site.page.clone(),
                chosen_by: site.chosen_by.clone().unwrap_or_default(),
                found_at: site
                    .checked_at
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            });
            report.services_given_website_logo += 1;
        }
    }

    if let Err(e) = opts
        .cache
        .write(serde_json::to_vec(&cache)?, "application/json")
        .await
    {
        tracing::warn!(error = %e, "could not save the logo cache; the next run will repeat lookups");
    }
    Ok(report)
}

/// The organization's site: its OrganizationURL, else the service's InformationURL.
fn site_of(s: &Service) -> Option<(String, String)> {
    let page = s
        .organization
        .urls
        .values()
        .chain(s.information_urls.values())
        .find(|u| u.starts_with("http://") || u.starts_with("https://"))?;
    let url = Url::parse(page.trim()).ok()?;
    let origin = format!("{}://{}", url.scheme(), url.host_str()?.to_lowercase());
    Some((origin.replacen("http://", "https://", 1), url.to_string()))
}

async fn lookup(http: &reqwest::Client, claude: &Claude, page: &str, org: &str) -> Site {
    let site = |logo: Option<String>,
                outcome: &str,
                detail: String,
                chosen_by: Option<String>,
                page: &str| Site {
        logo,
        page: page.to_owned(),
        chosen_by,
        outcome: outcome.into(),
        detail,
        checked_at: Utc::now(),
    };
    // Try the page over HTTPS first; many metadata URLs still say http://.
    let https = page.replacen("http://", "https://", 1);
    let fetched = match fetch_page(http, &https).await {
        Ok(p) => Ok(p),
        Err(_) if https != page => fetch_page(http, page).await,
        Err(e) => Err(e),
    };
    let (final_url, html) = match fetched {
        Ok(p) => p,
        Err(e) => return site(None, "error", format!("{e:#}"), None, page),
    };
    let (title, candidates) = candidates(&final_url, &html);
    if candidates.is_empty() {
        return site(
            None,
            "none",
            "no candidate images on the page".into(),
            None,
            final_url.as_str(),
        );
    }
    let (choices, model, reason) = match choose(claude, org, &final_url, &title, &candidates).await
    {
        Ok(c) => c,
        Err(e) => {
            return site(
                None,
                "error",
                format!("Claude: {e:#}"),
                None,
                final_url.as_str(),
            )
        }
    };
    for i in choices {
        let Some(c) = candidates.get(i) else { continue };
        // Logos are shown on an HTTPS page, so only an https URL that works will do.
        let url = c.url.replacen("http://", "https://", 1);
        if let ImageCheck::Ok = check_image(http, &url, true).await {
            return site(Some(url), "found", reason, Some(model), final_url.as_str());
        }
    }
    site(
        None,
        "none",
        format!("no usable image among the choices ({reason})"),
        Some(model),
        final_url.as_str(),
    )
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Candidate {
    pub url: String,
    /// Where on the page it came from, e.g. `link rel="apple-touch-icon"`.
    pub found_in: String,
    pub size: Option<String>,
    pub hint: Option<String>,
}

/// Candidate logo images on a page, in page order, deduplicated.
pub(crate) fn candidates(page: &Url, html: &str) -> (String, Vec<Candidate>) {
    use scraper::{Html, Selector};
    let doc = Html::parse_document(html);
    let sel = |s: &str| Selector::parse(s).expect("static selector");
    let base = doc
        .select(&sel("base[href]"))
        .next()
        .and_then(|b| b.value().attr("href"))
        .and_then(|h| page.join(h).ok())
        .unwrap_or_else(|| page.clone());
    let title = doc
        .select(&sel("title"))
        .next()
        .map(|t| {
            t.text()
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();

    let mut out: Vec<Candidate> = Vec::new();
    let mut push = |href: &str, found_in: String, size: Option<String>, hint: Option<String>| {
        let href = href.trim();
        if href.is_empty() || href.starts_with("data:") || out.len() >= MAX_CANDIDATES {
            return;
        }
        let Ok(url) = base.join(href) else { return };
        if !matches!(url.scheme(), "http" | "https") || out.iter().any(|c| c.url == url.as_str()) {
            return;
        }
        let short = |s: Option<&str>| {
            s.map(|v| v.chars().take(80).collect::<String>())
                .filter(|v| !v.is_empty())
        };
        out.push(Candidate {
            url: url.to_string(),
            found_in,
            size: short(size.as_deref()),
            hint: short(hint.as_deref()),
        });
    };

    for el in doc.select(&sel("link[rel][href]")) {
        let rel = el.value().attr("rel").unwrap_or("").to_lowercase();
        if rel.contains("icon") {
            push(
                el.value().attr("href").unwrap(),
                format!("link rel=\"{rel}\""),
                el.value().attr("sizes").map(str::to_owned),
                None,
            );
        }
    }
    for (selector, label) in [
        ("meta[itemprop=logo][content]", "meta itemprop=logo"),
        ("meta[property='og:logo'][content]", "meta og:logo"),
        ("meta[property='og:image'][content]", "meta og:image"),
        ("meta[name='twitter:image'][content]", "meta twitter:image"),
    ] {
        for el in doc.select(&sel(selector)) {
            push(
                el.value().attr("content").unwrap(),
                label.into(),
                None,
                None,
            );
        }
    }
    for el in doc.select(&sel("script[type='application/ld+json']")) {
        let text: String = el.text().collect();
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            for logo in json_ld_logos(&v) {
                push(&logo, "schema.org logo".into(), None, None);
            }
        }
    }
    let mut imgs = 0;
    for el in doc.select(&sel("img")) {
        let v = el.value();
        let src = v.attr("src").or_else(|| v.attr("data-src")).unwrap_or("");
        let alt = v.attr("alt").unwrap_or("");
        let marks = [
            src,
            alt,
            v.attr("class").unwrap_or(""),
            v.attr("id").unwrap_or(""),
        ]
        .join(" ")
        .to_lowercase();
        let in_header = el.ancestors().any(|a| {
            a.value().as_element().is_some_and(|e| {
                matches!(e.name(), "header")
                    || e.attr("class")
                        .is_some_and(|c| c.to_lowercase().contains("logo"))
            })
        });
        if (marks.contains("logo") || in_header) && imgs < 12 {
            imgs += 1;
            let size = match (v.attr("width"), v.attr("height")) {
                (Some(w), Some(h)) => Some(format!("{w}x{h}")),
                _ => None,
            };
            push(
                src,
                if in_header {
                    "img in header".into()
                } else {
                    "img".into()
                },
                size,
                Some(alt.to_owned()),
            );
        }
    }
    // Conventional locations, often present without a link tag.
    push(
        "/apple-touch-icon.png",
        "conventional path (unverified)".into(),
        Some("180x180".into()),
        None,
    );
    push(
        "/favicon.ico",
        "conventional path (unverified)".into(),
        None,
        None,
    );
    (title, out)
}

fn json_ld_logos(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    match v {
        Value::Object(m) => {
            for (k, val) in m {
                if k == "logo" {
                    match val {
                        Value::String(s) => out.push(s.clone()),
                        Value::Object(o) => {
                            if let Some(Value::String(s)) =
                                o.get("url").or_else(|| o.get("contentUrl"))
                            {
                                out.push(s.clone());
                            }
                        }
                        _ => {}
                    }
                } else {
                    out.extend(json_ld_logos(val));
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| out.extend(json_ld_logos(x))),
        _ => {}
    }
    out
}

const CHOOSER_SYSTEM: &str = "You pick an organization's logo from the images found on its website. The logo is shown as a small square tile beside the organization's services in a catalog of research and education services.

You get the organization's name, the page, and a numbered list of candidate images, each with where on the page it came from and any declared size or alt text. Return up to three candidate numbers, best first.

Prefer, in order: an SVG or large image of the organization's own logo or emblem; an apple-touch-icon or large site icon, which is usually the logo mark; a small favicon only when nothing better exists. Never choose photographs, banners, hero images, social-media icons, partner or sponsor logos, award or accreditation badges, tracking pixels, or another organization's mark. Candidates marked unverified may not exist; rank them below real ones. If no candidate is plausibly this organization's logo, return an empty list.

`reason`: one short sentence on the choice.";

async fn choose(
    claude: &Claude,
    org: &str,
    page: &Url,
    title: &str,
    candidates: &[Candidate],
) -> Result<(Vec<usize>, String, String)> {
    let mut user =
        format!("Organization: {org}\nPage: {page}\nPage title: {title}\n\nCandidates:\n");
    for (i, c) in candidates.iter().enumerate() {
        user.push_str(&format!("{i}. {} — from {}", c.url, c.found_in));
        if let Some(s) = &c.size {
            user.push_str(&format!(", size {s}"));
        }
        if let Some(h) = &c.hint {
            user.push_str(&format!(", alt \"{h}\""));
        }
        user.push('\n');
    }
    // Indices are range-checked below rather than enumerated in the schema, so the
    // schema is identical on every call and compiles once.
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["choices", "reason"],
        "properties": {
            "choices": { "type": "array", "items": { "type": "integer" } },
            "reason": { "type": "string" },
        },
    });
    let answer = claude
        .structured(CHOOSER_SYSTEM, &user, schema, 2048)
        .await?;
    let choices: Vec<usize> = answer.value["choices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .map(|i| i as usize)
        .filter(|i| *i < candidates.len())
        .take(3)
        .collect();
    let reason = answer.value["reason"].as_str().unwrap_or("").to_owned();
    Ok((choices, answer.model, reason))
}

#[derive(Debug)]
pub(crate) enum ImageCheck {
    Ok,
    Broken(String),
    Unknown(String),
}

/// Fetch `url` and decide whether it is an image a browser can show. `strict`
/// (website logos) also rejects tiny or oversized images and treats inconclusive
/// answers as failures; metadata logos get the benefit of the doubt.
pub(crate) async fn check_image(http: &reqwest::Client, url: &str, strict: bool) -> ImageCheck {
    let resp = match http
        .get(url)
        .header("Accept", "image/*,*/*;q=0.5")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) if e.is_timeout() => return ImageCheck::Unknown("timeout".into()),
        Err(e) => return ImageCheck::Broken(format!("unreachable: {}", root_cause(&e))),
    };
    let status = resp.status();
    if matches!(status, StatusCode::NOT_FOUND | StatusCode::GONE) {
        return ImageCheck::Broken(format!("HTTP {status}"));
    }
    if !status.is_success() {
        let d = format!("HTTP {status}");
        return if strict {
            ImageCheck::Broken(d)
        } else {
            ImageCheck::Unknown(d)
        };
    }
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    let body = match read_limited(resp, MAX_IMAGE_BYTES).await {
        Ok((b, false)) => b,
        Ok((_, true)) if strict => return ImageCheck::Broken("larger than 1 MiB".into()),
        Ok((b, true)) => b,
        Err(e) => return ImageCheck::Unknown(format!("read failed: {e}")),
    };
    let sniffed = sniff(&body);
    if !ctype.starts_with("image/") && sniffed.is_none() {
        return ImageCheck::Broken(format!(
            "not an image ({})",
            if ctype.is_empty() {
                "no content type"
            } else {
                &ctype
            }
        ));
    }
    let dims = sniffed.flatten();
    if strict {
        if let Some((w, h)) = dims {
            if w.max(h) < MIN_PIXELS {
                return ImageCheck::Broken(format!("too small ({w}x{h})"));
            }
        }
    }
    ImageCheck::Ok
}

/// `Some(dims)` when the bytes are a recognizable image; dims when the format
/// states them cheaply (PNG, GIF, ICO).
pub(crate) fn sniff(b: &[u8]) -> Option<Option<(u32, u32)>> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") && b.len() >= 24 {
        let w = u32::from_be_bytes([b[16], b[17], b[18], b[19]]);
        let h = u32::from_be_bytes([b[20], b[21], b[22], b[23]]);
        return Some(Some((w, h)));
    }
    if b.starts_with(b"GIF8") && b.len() >= 10 {
        return Some(Some((
            u16::from_le_bytes([b[6], b[7]]) as u32,
            u16::from_le_bytes([b[8], b[9]]) as u32,
        )));
    }
    if b.starts_with(&[0, 0, 1, 0]) && b.len() >= 8 {
        let side = |x: u8| if x == 0 { 256 } else { x as u32 };
        return Some(Some((side(b[6]), side(b[7]))));
    }
    if b.starts_with(&[0xFF, 0xD8, 0xFF])
        || (b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP"))
    {
        return Some(None);
    }
    let head = String::from_utf8_lossy(&b[..b.len().min(512)]).to_lowercase();
    if head.contains("<svg") {
        return Some(None);
    }
    None
}

async fn fetch_page(http: &reqwest::Client, url: &str) -> Result<(Url, String)> {
    let url = Url::parse(url)?;
    if !allowed(&url) {
        bail!("refusing to fetch {url}");
    }
    let resp = http
        .get(url)
        .header("Accept", "text/html,*/*;q=0.5")
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("HTTP {}", resp.status());
    }
    let final_url = resp.url().clone();
    let (body, _) = read_limited(resp, MAX_PAGE_BYTES).await?;
    Ok((final_url, String::from_utf8_lossy(&body).into_owned()))
}

async fn read_limited(mut resp: reqwest::Response, limit: usize) -> Result<(Vec<u8>, bool)> {
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if out.len() + chunk.len() > limit {
            out.extend_from_slice(&chunk[..limit - out.len()]);
            return Ok((out, true));
        }
        out.extend_from_slice(&chunk);
    }
    Ok((out, false))
}

fn root_cause(e: &reqwest::Error) -> String {
    let mut src: &dyn std::error::Error = e;
    while let Some(next) = src.source() {
        src = next;
    }
    src.to_string()
}

// ---------- the guarded client ----------

fn guarded_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(6))
        .timeout(Duration::from_secs(12))
        .dns_resolver(Arc::new(PublicOnly))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                attempt.error("too many redirects")
            } else if allowed(attempt.url()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()?)
}

/// http(s) on the default ports, and never a literal private address. Host names
/// are screened by [`PublicOnly`] when they resolve.
pub(crate) fn allowed(url: &Url) -> bool {
    let port_ok = matches!(url.port(), None | Some(80) | Some(443));
    let host_ok = match url.host() {
        Some(url::Host::Ipv4(ip)) => is_public(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => is_public(IpAddr::V6(ip)),
        Some(url::Host::Domain(d)) => !d.eq_ignore_ascii_case("localhost"),
        None => false,
    };
    matches!(url.scheme(), "http" | "https") && port_ok && host_ok
}

pub(crate) fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local() // includes 169.254.169.254, the cloud metadata service
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xC0) == 64) // 100.64.0.0/10, carrier-grade NAT
                || (o[0] == 198 && (o[1] & 0xFE) == 18) // 198.18.0.0/15, benchmarking
                || o[0] >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let seg = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (seg[0] & 0xFE00) == 0xFC00 // unique local
                || (seg[0] & 0xFFC0) == 0xFE80) // link local
        }
    }
}

struct PublicOnly;

impl reqwest::dns::Resolve for PublicOnly {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let host = name.as_str().to_owned();
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|a| is_public(a.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(format!("{host} has no public address").into());
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_private_and_metadata_addresses() {
        for bad in [
            "http://169.254.169.254/latest/meta-data/",
            "http://127.0.0.1/",
            "http://10.0.0.5/logo.png",
            "http://[::1]/",
            "http://[fd00::1]/",
            "http://localhost/",
            "https://example.org:8443/logo.png",
            "file:///etc/passwd",
        ] {
            assert!(!allowed(&Url::parse(bad).unwrap()), "{bad}");
        }
        assert!(allowed(
            &Url::parse("https://www.example.org/logo.svg").unwrap()
        ));
        assert!(!is_public("100.64.1.1".parse().unwrap()));
        assert!(is_public("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn finds_candidates_and_resolves_relative_urls() {
        let page = Url::parse("https://www.example.edu/about/").unwrap();
        let html = r#"<html><head><title> Example
            University </title>
            <link rel="icon" href="/favicon-32.png" sizes="32x32">
            <link rel="apple-touch-icon" href="https://cdn.example.edu/touch.png">
            <meta property="og:image" content="/img/campus-photo.jpg">
            <script type="application/ld+json">{"@type":"CollegeOrUniversity","logo":{"@type":"ImageObject","url":"/brand/logo.svg"}}</script>
            </head><body><header><img src="brand/wordmark.png" alt="Example University"></header>
            <img src="/photos/students.jpg" alt="students"><img src="data:image/png;base64,AAAA" class="logo"></body></html>"#;
        let (title, c) = candidates(&page, html);
        assert_eq!(title, "Example University");
        let urls: Vec<&str> = c.iter().map(|c| c.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://www.example.edu/favicon-32.png",
                "https://cdn.example.edu/touch.png",
                "https://www.example.edu/img/campus-photo.jpg",
                "https://www.example.edu/brand/logo.svg",
                "https://www.example.edu/about/brand/wordmark.png",
                "https://www.example.edu/apple-touch-icon.png",
                "https://www.example.edu/favicon.ico",
            ]
        );
        assert_eq!(c[0].size.as_deref(), Some("32x32"));
        assert_eq!(c[4].found_in, "img in header");
    }

    #[test]
    fn sniffs_formats_and_sizes() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&180u32.to_be_bytes());
        png.extend_from_slice(&90u32.to_be_bytes());
        assert_eq!(sniff(&png), Some(Some((180, 90))));
        assert_eq!(sniff(&[0, 0, 1, 0, 1, 0, 16, 16]), Some(Some((16, 16))));
        assert_eq!(sniff(b"<?xml version=\"1.0\"?><svg xmlns=..."), Some(None));
        assert_eq!(sniff(b"<!doctype html><html>Not found</html>"), None);
    }

    #[test]
    fn site_prefers_the_organization_url() {
        let mut s = Service::default();
        s.organization
            .urls
            .insert("en".into(), "http://www.Example.edu/".into());
        s.information_urls
            .insert("en".into(), "https://app.example.com/info".into());
        assert_eq!(site_of(&s).unwrap().0, "https://www.example.edu");
    }
}
