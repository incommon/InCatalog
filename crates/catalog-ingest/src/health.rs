// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Check that services still answer, and label them alive, dead or unknown.
//!
//! Two kinds of check, at most one request each, through the same guarded client as
//! the logo checks (public addresses only, default ports only):
//!
//! - **Login hosts** (`endpoint_hosts`): a `HEAD https://<host>/`. Any HTTP answer
//!   counts, whatever its status, because a login endpoint expects a SAML POST and
//!   many answer anything else with an error. What counts is DNS, the connection,
//!   and TLS. How the answers add up is [`catalog_core::health`].
//! - **Pages meant to be read** (information, organization and privacy URLs): a
//!   `HEAD`, falling back to `GET` when the site refuses `HEAD`. A 404 or 410 makes
//!   it a broken link; 403, 429 and 5xx say nothing.
//!
//! Results are cached (`health-cache.json` beside the snapshot) and rechecked on a
//! schedule spread across runs, not all at once: hosts that answered every
//! [`RECHECK_OK`], failing or unclear ones every [`RECHECK_FAILING`], so a failing
//! host collects its checks over several days.
//!
//! Checks run in a random order, login hosts first. When many in a row can't reach
//! their host, the run asks a host that answered earlier before it stops for a
//! network that looks down ([`crate::logos::Breaker`]): in name order, one
//! organization's retired hosts came in a row, and on 2026-11-01 stopped 2,683 of a
//! run's 4,000 checks.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::BuildHasher;

use anyhow::Result;
use catalog_core::health::{self, Outcome};
use catalog_core::model::{BrokenLink, HealthStatus, HostHealth, Service};
use catalog_store::Location;
use chrono::{DateTime, Duration, Utc};
use futures::stream::{self, StreamExt};
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};

use crate::budget::Deadline;
use crate::logos::{allowed, guarded_client_as, Breaker, Failure};

const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (compatible; InCatalogHealthCheck/",
    env!("CARGO_PKG_VERSION"),
    ")"
);
/// How often a host or page that answered is checked again.
pub const RECHECK_OK: Duration = Duration::hours(72);
/// How often a failing or unclear one is: about daily, so a failure is confirmed
/// over several days rather than several runs in one.
pub const RECHECK_FAILING: Duration = Duration::hours(20);
/// Checks in flight at once.
const AT_ONCE: usize = 16;

pub struct Options {
    pub cache: Location,
    /// Most hosts and pages to check per run; the rest wait for the next.
    pub max_checks: usize,
    pub deadline: Deadline,
    /// A URL that answered earlier in the run (the InCommon aggregate), asked before
    /// checks stop for a network that looks down.
    pub canary: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub hosts_checked: usize,
    pub links_checked: usize,
    /// Due, but over this run's cap; they wait for the next run.
    pub waiting: usize,
    pub stopped_for_time: usize,
    /// Not started because checks in a row could not reach their host, and neither
    /// could the canary.
    pub skipped_network_down: usize,
    /// Services by status after this run.
    pub services: BTreeMap<String, usize>,
    /// Login hosts whose latest check failed, by why.
    pub host_failures: BTreeMap<String, usize>,
    pub broken_links: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    #[serde(default)]
    hosts: BTreeMap<String, HostHealth>,
    #[serde(default)]
    links: BTreeMap<String, Link>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Link {
    /// `ok`, `broken`, or `unclear`.
    status: String,
    detail: String,
    checked_at: DateTime<Utc>,
    /// The first check that found it broken, in the current run of broken checks.
    since: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LinkOutcome {
    Ok,
    Broken(String),
    Unclear(String),
}

/// What to check, in priority order: login hosts first.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Job {
    Host(String),
    Link(String),
}

pub async fn check(services: &mut [Service], opts: Options) -> Result<Report> {
    check_with(services, opts, guarded_client_as(USER_AGENT)?, "https").await
}

/// `scheme` is `https` except in tests, which serve plain HTTP.
async fn check_with(
    services: &mut [Service],
    opts: Options,
    http: reqwest::Client,
    scheme: &'static str,
) -> Result<Report> {
    let now = Utc::now();
    let mut cache: Cache = match opts.cache.read(None).await {
        Ok(Some(blob)) => serde_json::from_slice(&blob.bytes).unwrap_or_default(),
        _ => Cache::default(),
    };
    let mut report = Report::default();

    let mut jobs: BTreeSet<Job> = BTreeSet::new();
    for s in services.iter() {
        for h in &s.endpoint_hosts {
            if due(cache.hosts.get(h).map(host_due_at), now) {
                jobs.insert(Job::Host(h.clone()));
            }
        }
        for (url, _) in links(s) {
            if due(cache.links.get(&url).map(link_due_at), now) {
                jobs.insert(Job::Link(url));
            }
        }
    }
    report.waiting = jobs.len().saturating_sub(opts.max_checks);
    // Login hosts first, each kind in a random order: in name order, one
    // organization's dead hosts come in a row and look like the network going down.
    let order = std::hash::RandomState::new();
    let mut to_run: Vec<Job> = jobs.into_iter().collect();
    to_run.sort_by_cached_key(|j| (matches!(j, Job::Link(_)), order.hash_one(j)));
    to_run.truncate(opts.max_checks);
    let started = to_run.len();

    let breaker = std::sync::Arc::new(Breaker::probing(&http, opts.canary.as_deref()));
    let results: Vec<(Job, Option<Outcome>, Option<LinkOutcome>)> = opts
        .deadline
        .until(stream::iter(to_run))
        .map(|job| {
            let (http, breaker) = (http.clone(), breaker.clone());
            async move {
                if breaker.network_down().await {
                    return (job, None, None);
                }
                match &job {
                    Job::Host(h) => {
                        let o = check_host(&http, scheme, h).await;
                        breaker.record(unreachable(&o).then_some(Failure::Dns));
                        (job, Some(o), None)
                    }
                    Job::Link(u) => {
                        let o = check_link(&http, u).await;
                        (job, None, Some(o))
                    }
                }
            }
        })
        .buffer_unordered(AT_ONCE)
        .collect()
        .await;
    report.stopped_for_time = started - results.len();

    for (job, host, link) in results {
        match (job, host, link) {
            (Job::Host(h), Some(o), _) => {
                report.hosts_checked += 1;
                let rec = cache.hosts.entry(h.clone()).or_insert_with(|| HostHealth {
                    host: h,
                    ..Default::default()
                });
                health::observe(rec, now, o);
            }
            (Job::Link(u), _, Some(o)) => {
                report.links_checked += 1;
                let prev_since = cache.links.get(&u).and_then(|l| l.since);
                let (status, detail, since) = match o {
                    LinkOutcome::Ok => ("ok", String::new(), None),
                    LinkOutcome::Broken(d) => ("broken", d, Some(prev_since.unwrap_or(now))),
                    LinkOutcome::Unclear(d) => ("unclear", d, prev_since),
                };
                cache.links.insert(
                    u,
                    Link {
                        status: status.into(),
                        detail,
                        checked_at: now,
                        since,
                    },
                );
            }
            _ => report.skipped_network_down += 1,
        }
    }

    for s in services.iter_mut() {
        let hosts: Vec<HostHealth> = s
            .endpoint_hosts
            .iter()
            .map(|h| {
                cache.hosts.get(h).cloned().unwrap_or_else(|| HostHealth {
                    host: h.clone(),
                    ..Default::default()
                })
            })
            .collect();
        let broken: Vec<BrokenLink> = links(s)
            .into_iter()
            .filter_map(|(url, kind)| {
                let l = cache.links.get(&url).filter(|l| l.status == "broken")?;
                Some(BrokenLink {
                    url,
                    kind: kind.into(),
                    detail: l.detail.clone(),
                    since: l
                        .since
                        .unwrap_or(l.checked_at)
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                })
            })
            .collect();
        report.broken_links += broken.len();
        let h = health::service_health(hosts, broken, now);
        *report
            .services
            .entry(h.status.key().to_owned())
            .or_default() += 1;
        s.health = Some(h);
    }
    for h in cache.hosts.values() {
        if health::host_status(h, now) != HealthStatus::Alive {
            if let Some(f) = &h.failure {
                *report.host_failures.entry(f.clone()).or_default() += 1;
            }
        }
    }

    opts.cache
        .write(serde_json::to_vec(&cache)?, "application/json")
        .await?;
    Ok(report)
}

/// A service's pages meant to be read, with which kind each is.
fn links(s: &Service) -> Vec<(String, &'static str)> {
    let mut out: Vec<(String, &'static str)> = Vec::new();
    for (map, kind) in [
        (&s.information_urls, "information"),
        (&s.organization.urls, "organization"),
        (&s.privacy_statement_urls, "privacy"),
    ] {
        for u in map.values() {
            let u = u.trim();
            if u.starts_with("https://")
                && Url::parse(u).is_ok_and(|p| allowed(&p))
                && !out.iter().any(|(x, _)| x == u)
            {
                out.push((u.to_owned(), kind));
            }
        }
    }
    out
}

fn host_due_at(h: &HostHealth) -> Option<DateTime<Utc>> {
    let at = h
        .checked_at
        .as_deref()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())?
        .with_timezone(&Utc);
    let answered = h.failure.is_none() && h.last_ok.is_some();
    Some(
        at + spread(
            &h.host,
            if answered {
                RECHECK_OK
            } else {
                RECHECK_FAILING
            },
        ),
    )
}

fn link_due_at(l: &Link) -> Option<DateTime<Utc>> {
    let every = if l.status == "ok" {
        RECHECK_OK
    } else {
        RECHECK_FAILING
    };
    Some(l.checked_at + every)
}

fn due(due_at: Option<Option<DateTime<Utc>>>, now: DateTime<Utc>) -> bool {
    match due_at.flatten() {
        Some(at) => at <= now,
        None => true,
    }
}

/// The interval, lengthened by up to a quarter depending on the host, so hosts first
/// checked in one run don't all come due in the same later run.
fn spread(key: &str, every: Duration) -> Duration {
    let h = key
        .bytes()
        .fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    every + every * (h % 25) as i32 / 100
}

async fn check_host(http: &reqwest::Client, scheme: &str, host: &str) -> Outcome {
    let url = format!("{scheme}://{host}/");
    match http.head(&url).send().await {
        Ok(_) => Outcome::Answered,
        Err(e) => classify(&e),
    }
}

async fn check_link(http: &reqwest::Client, url: &str) -> LinkOutcome {
    let mut resp = http.head(url).send().await;
    // Some sites refuse HEAD, or answer it differently from GET.
    if let Ok(r) = &resp {
        if !r.status().is_success() && !r.status().is_redirection() {
            resp = http.get(url).send().await;
        }
    }
    match resp {
        Ok(r) => match r.status() {
            s if s.is_success() || s.is_redirection() => LinkOutcome::Ok,
            s @ (StatusCode::NOT_FOUND | StatusCode::GONE) => {
                LinkOutcome::Broken(format!("HTTP {}", s.as_u16()))
            }
            s => LinkOutcome::Unclear(format!("HTTP {}", s.as_u16())),
        },
        Err(e) => match classify(&e) {
            Outcome::Failed {
                detail,
                evidence: true,
                ..
            } => LinkOutcome::Broken(detail),
            Outcome::Failed { detail, .. } => LinkOutcome::Unclear(detail),
            Outcome::Answered => LinkOutcome::Ok,
        },
    }
}

/// Never reached the host: what a network outage looks like too.
fn unreachable(o: &Outcome) -> bool {
    matches!(o, Outcome::Failed { kind, .. } if kind == health::DEAD_AT_ONCE || kind == "dns" || kind == "dns_transient" || kind == "connect")
}

fn classify(e: &reqwest::Error) -> Outcome {
    let mut src: &dyn std::error::Error = e;
    let mut text = e.to_string();
    while let Some(next) = src.source() {
        src = next;
        text.push_str(": ");
        text.push_str(&next.to_string());
    }
    let (kind, evidence) = if e.is_timeout() {
        ("timeout", false)
    } else {
        kind_of(&text)
    };
    Outcome::Failed {
        kind: kind.into(),
        detail: src.to_string(),
        evidence,
    }
}

/// Which failure an error's text describes, and whether it counts as evidence.
pub(crate) fn kind_of(text: &str) -> (&'static str, bool) {
    let t = text.to_lowercase();
    let any = |ks: &[&str]| ks.iter().any(|k| t.contains(k));
    if any(&["has no public address"]) {
        ("private_address", false)
    } else if any(&["nxdomain"]) {
        (health::DEAD_AT_ONCE, true)
    } else if any(&[
        "nodename nor servname",
        "name or service not known",
        "no address associated",
    ]) {
        ("dns", true)
    } else if any(&[
        "failed to lookup address",
        "dns error",
        "temporary failure in name resolution",
    ]) {
        ("dns_transient", false)
    } else if any(&["expired"]) && any(&["certificate"]) {
        ("tls_expired", true)
    } else if any(&["notvalidforname", "not valid for name"]) {
        ("tls_name", true)
    } else if any(&[
        "unknownissuer",
        "unknown issuer",
        "self signed",
        "selfsigned",
    ]) {
        ("tls_untrusted", true)
    } else if any(&["certificate", "tls", "handshake", "ssl"]) {
        ("tls", true)
    } else if any(&["connection refused"]) {
        ("connect", true)
    } else if any(&["timed out", "timeout"]) {
        ("timeout", false)
    } else {
        ("other", false)
    }
}

/// Runs against a stand-in website.
#[cfg(test)]
mod tests {
    use axum::http::{StatusCode, Uri};
    use axum::response::IntoResponse;
    use axum::Router;

    use super::*;

    #[test]
    fn classifies_failures_and_says_which_count() {
        for (text, want) in [
            ("error sending request: dns error: failed to lookup address information: nodename nor servname provided, or not known", ("dns", true)),
            ("dns error: failed to lookup address information: Name or service not known", ("dns", true)),
            ("dns error: failed to lookup address information: Temporary failure in name resolution", ("dns_transient", false)),
            ("dns error: intranet.example.edu has no public address", ("private_address", false)),
            ("dns error: retired.example.edu: no such name (NXDOMAIN)", ("nxdomain", true)),
            ("dns error: cname.example.edu: no address associated with the name", ("dns", true)),
            ("dns error: sp.example.edu: temporary failure in name resolution: request timed out", ("dns_transient", false)),
            ("invalid peer certificate: certificate expired: verification time 1793558769 (UNIX), but certificate is not valid after 1790000000 (UNIX)", ("tls_expired", true)),
            ("invalid peer certificate: certificate not valid for name \"sp.example.org\"; certificate is only valid for DnsName(\"other.example.org\")", ("tls_name", true)),
            ("invalid peer certificate: UnknownIssuer", ("tls_untrusted", true)),
            ("received fatal alert: HandshakeFailure", ("tls", true)),
            ("tcp connect error: Connection refused (os error 61)", ("connect", true)),
            ("connection reset by peer", ("other", false)),
        ] {
            assert_eq!(kind_of(text), want, "{text}");
        }
    }

    #[test]
    fn spreads_rechecks_by_up_to_a_quarter() {
        for h in ["a.example", "sp.example.org", "login.example.edu"] {
            let d = spread(h, RECHECK_OK);
            assert!(d >= RECHECK_OK && d <= RECHECK_OK * 5 / 4, "{h}: {d}");
        }
    }

    /// `/` answers 500 (a login endpoint refusing a plain request), `/gone` 404,
    /// `/flaky` 503, `/no-head` 405 to HEAD and 200 to GET.
    async fn stub() -> String {
        let app = Router::new().fallback(|method: axum::http::Method, uri: Uri| async move {
            match (method.as_str(), uri.path()) {
                (_, "/") => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                (_, "/gone") => StatusCode::NOT_FOUND.into_response(),
                (_, "/flaky") => StatusCode::SERVICE_UNAVAILABLE.into_response(),
                ("HEAD", "/no-head") => StatusCode::METHOD_NOT_ALLOWED.into_response(),
                (_, "/no-head") => "fine".into_response(),
                _ => StatusCode::OK.into_response(),
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr.to_string()
    }

    fn plain() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn checks_hosts_and_pages() {
        let addr = stub().await;
        let http = plain();
        assert_eq!(
            check_host(&http, "http", &addr).await,
            Outcome::Answered,
            "a 500 from a login host still counts as answering"
        );
        let refused = check_host(&http, "http", "127.0.0.1:9").await;
        assert!(
            matches!(refused, Outcome::Failed { ref kind, evidence: true, .. } if kind == "connect"),
            "{refused:?}"
        );

        let page = |p: &str| format!("http://{addr}{p}");
        assert_eq!(check_link(&http, &page("/ok")).await, LinkOutcome::Ok);
        assert_eq!(check_link(&http, &page("/no-head")).await, LinkOutcome::Ok);
        assert_eq!(
            check_link(&http, &page("/gone")).await,
            LinkOutcome::Broken("HTTP 404".into())
        );
        assert_eq!(
            check_link(&http, &page("/flaky")).await,
            LinkOutcome::Unclear("HTTP 503".into())
        );
    }

    #[tokio::test]
    async fn a_run_labels_services_caches_and_waits_its_turn() {
        let addr = stub().await;
        let path =
            std::env::temp_dir().join(format!("incatalog-health-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let svc = |id: &str, host: &str, info: Option<String>| {
            let mut s = Service {
                entity_id: id.into(),
                endpoint_hosts: vec![host.into()],
                ..Default::default()
            };
            if let Some(i) = info {
                s.information_urls.insert("en".into(), i);
            }
            s
        };
        // Pages must be https to be checked, so the broken page here is never
        // fetched; the plain-HTTP stub stands in for login hosts only.
        let mut services = vec![
            svc("https://up/", &addr, Some(format!("http://{addr}/gone"))),
            svc("https://down/", "127.0.0.1:9", None),
        ];
        let opts = |max| Options {
            cache: Location::parse(path.to_str().unwrap()).unwrap(),
            max_checks: max,
            deadline: Deadline::none(),
            canary: None,
        };
        let r = check_with(&mut services, opts(10), plain(), "http")
            .await
            .unwrap();
        assert_eq!((r.hosts_checked, r.links_checked, r.waiting), (2, 0, 0));
        assert_eq!(
            services[0].health.as_ref().unwrap().status,
            HealthStatus::Alive
        );
        let down = services[1].health.as_ref().unwrap();
        assert_eq!(
            down.status,
            HealthStatus::Unknown,
            "one failure is not dead"
        );
        assert_eq!(down.hosts[0].failure.as_deref(), Some("connect"));
        assert_eq!(r.services.get("alive"), Some(&1));
        assert_eq!(r.host_failures.get("connect"), Some(&1));

        // Rechecked only when due: a second run right away checks nothing, but still
        // labels every service from the cache.
        services.iter_mut().for_each(|s| s.health = None);
        let r = check_with(&mut services, opts(10), plain(), "http")
            .await
            .unwrap();
        assert_eq!(r.hosts_checked, 0);
        assert_eq!(
            services[0].health.as_ref().unwrap().status,
            HealthStatus::Alive
        );

        // Over the cap, the rest wait.
        let _ = std::fs::remove_file(&path);
        let r = check_with(&mut services, opts(1), plain(), "http")
            .await
            .unwrap();
        assert_eq!((r.hosts_checked, r.waiting), (1, 1));
        let _ = std::fs::remove_file(&path);
    }

    /// Forty dead hosts are not an outage while the canary answers: every one is
    /// checked. When the canary is dead too, the run stops early.
    #[tokio::test]
    async fn dead_hosts_in_a_row_stop_the_run_only_when_the_canary_fails_too() {
        let addr = stub().await;
        // Closed ports: each connection is refused, as for a retired host. A port is
        // free again once its listener drops, so the system can hand it out twice;
        // two services on one host are one check, and the run would count 39.
        let mut closed: Vec<String> = Vec::new();
        while closed.len() < 40 {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = l.local_addr().unwrap().to_string();
            if !closed.contains(&port) {
                closed.push(port);
            }
        }
        let services = || -> Vec<Service> {
            closed
                .iter()
                .map(|h| Service {
                    entity_id: format!("https://{h}/"),
                    endpoint_hosts: vec![h.clone()],
                    ..Default::default()
                })
                .collect()
        };
        for (canary, all_checked) in [
            (format!("http://{addr}/"), true),
            (format!("http://{}/", closed[0]), false),
        ] {
            let path = std::env::temp_dir().join(format!(
                "incatalog-health-breaker-{}-{all_checked}.json",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&path);
            let r = check_with(
                &mut services(),
                Options {
                    cache: Location::parse(path.to_str().unwrap()).unwrap(),
                    max_checks: 100,
                    deadline: Deadline::none(),
                    canary: Some(canary.clone()),
                },
                plain(),
                "http",
            )
            .await
            .unwrap();
            let _ = std::fs::remove_file(&path);
            if all_checked {
                assert_eq!(
                    (r.hosts_checked, r.skipped_network_down),
                    (40, 0),
                    "{canary}"
                );
            } else {
                assert!(r.skipped_network_down > 0, "{canary}: {r:?}");
                assert_eq!(r.hosts_checked + r.skipped_network_down, 40);
            }
        }
    }

    #[test]
    fn lists_only_https_pages_once_each() {
        let mut s = Service::default();
        s.information_urls
            .insert("en".into(), "https://example.org/about".into());
        s.information_urls
            .insert("de".into(), "https://example.org/about".into());
        s.privacy_statement_urls
            .insert("en".into(), "http://example.org/privacy".into());
        s.organization
            .urls
            .insert("en".into(), "https://example.org:8443/".into());
        assert_eq!(
            links(&s),
            [("https://example.org/about".to_owned(), "information")]
        );
    }
}
