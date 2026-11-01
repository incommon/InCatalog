// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Whether a service still answers: alive, dead, or unknown.
//!
//! Ingest checks each service's login hosts (`endpoint_hosts`, the
//! `AssertionConsumerService` hosts) for DNS, a connection and valid TLS, and never
//! by HTTP status: a login endpoint expects a SAML POST, so many answer a plain
//! request with 400, 405 or 500 while working fine. This module is the bookkeeping:
//! how one check changes a host's record, when a host counts as alive or dead, and
//! how a service's hosts add up.
//!
//! - **Alive is a positive finding.** A host is alive when its latest check
//!   answered and that was within [`ALIVE_FOR_DAYS`].
//! - **One bad check is not a dead host.** Only evidence counts toward dead: a name
//!   that doesn't exist, a refused connection, TLS that doesn't validate. A host is
//!   dead after [`DEAD_AFTER_FAILURES`] of those in a row spanning at least
//!   [`DEAD_AFTER_HOURS`]. Timeouts, temporary DNS failures and names that resolve
//!   only to private addresses are no clear answer: some services answer only from
//!   their campus, and a failure seen from outside isn't proof.
//! - **A service** is alive when any login host is, dead when every one is, and
//!   unknown otherwise, including when it lists none.

use chrono::{DateTime, Duration, Utc};

use crate::model::{BrokenLink, Health, HealthStatus, HostHealth};

/// Checks in a row that must find evidence before a host counts as dead.
pub const DEAD_AFTER_FAILURES: u32 = 3;
/// The span those failures must cover, so one bad day isn't enough.
pub const DEAD_AFTER_HOURS: i64 = 72;
/// How long a successful check keeps a host alive without another.
pub const ALIVE_FOR_DAYS: i64 = 14;

/// What one check of a host found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Answered,
    /// `evidence` failures count toward dead; the others are no clear answer.
    Failed {
        kind: String,
        detail: String,
        evidence: bool,
    },
}

/// Record one check on a host. A success clears the run of failures.
pub fn observe(h: &mut HostHealth, at: DateTime<Utc>, outcome: Outcome) {
    let at_s = at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    match outcome {
        Outcome::Answered => {
            h.last_ok = Some(at_s.clone());
            h.first_failed = None;
            h.failures_in_a_row = 0;
            h.failure = None;
            h.detail = None;
        }
        Outcome::Failed {
            kind,
            detail,
            evidence,
        } => {
            if evidence {
                h.first_failed.get_or_insert_with(|| at_s.clone());
                h.failures_in_a_row += 1;
            }
            h.failure = Some(kind);
            h.detail = Some(detail);
        }
    }
    h.checked_at = Some(at_s);
}

fn parse(t: &Option<String>) -> Option<DateTime<Utc>> {
    t.as_deref()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|t| t.with_timezone(&Utc))
}

/// A host's status as of `now`.
pub fn host_status(h: &HostHealth, now: DateTime<Utc>) -> HealthStatus {
    let recent = parse(&h.last_ok).is_some_and(|t| now - t <= Duration::days(ALIVE_FOR_DAYS));
    if h.failure.is_none() && recent {
        return HealthStatus::Alive;
    }
    let span = match (parse(&h.first_failed), parse(&h.checked_at)) {
        (Some(first), Some(last)) => last - first,
        _ => Duration::zero(),
    };
    if h.failures_in_a_row >= DEAD_AFTER_FAILURES && span >= Duration::hours(DEAD_AFTER_HOURS) {
        HealthStatus::Dead
    } else {
        HealthStatus::Unknown
    }
}

/// A service's health from its hosts' records, as of `now`.
pub fn service_health(
    mut hosts: Vec<HostHealth>,
    broken_links: Vec<BrokenLink>,
    now: DateTime<Utc>,
) -> Health {
    for h in &mut hosts {
        h.status = host_status(h, now);
    }
    let status = if hosts.iter().any(|h| h.status == HealthStatus::Alive) {
        HealthStatus::Alive
    } else if !hosts.is_empty() && hosts.iter().all(|h| h.status == HealthStatus::Dead) {
        HealthStatus::Dead
    } else {
        HealthStatus::Unknown
    };
    let since = (status == HealthStatus::Dead)
        .then(|| hosts.iter().filter_map(|h| h.first_failed.clone()).min())
        .flatten();
    let last_ok = hosts.iter().filter_map(|h| h.last_ok.clone()).max();
    Health {
        status,
        since,
        last_ok,
        hosts,
        broken_links,
    }
}

/// A service's status, `unknown` when it has never been checked.
pub fn status(s: &crate::model::Service) -> HealthStatus {
    s.health.as_ref().map(|h| h.status).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(day: i64, hour: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
            + Duration::days(day)
            + Duration::hours(hour)
    }

    fn fail(kind: &str, evidence: bool) -> Outcome {
        Outcome::Failed {
            kind: kind.into(),
            detail: format!("{kind} detail"),
            evidence,
        }
    }

    #[test]
    fn alive_needs_a_recent_answer() {
        let mut h = HostHealth::default();
        assert_eq!(
            host_status(&h, t(0, 0)),
            HealthStatus::Unknown,
            "never checked"
        );
        observe(&mut h, t(0, 0), Outcome::Answered);
        assert_eq!(host_status(&h, t(0, 1)), HealthStatus::Alive);
        assert_eq!(
            host_status(&h, t(ALIVE_FOR_DAYS + 1, 0)),
            HealthStatus::Unknown,
            "stale"
        );
    }

    #[test]
    fn dead_needs_several_failures_over_several_days_and_clears_on_success() {
        let mut h = HostHealth::default();
        observe(&mut h, t(0, 0), Outcome::Answered);
        observe(&mut h, t(1, 0), fail("dns", true));
        assert_eq!(
            host_status(&h, t(1, 0)),
            HealthStatus::Unknown,
            "one failure"
        );
        observe(&mut h, t(1, 6), fail("dns", true));
        observe(&mut h, t(1, 12), fail("dns", true));
        assert_eq!(
            host_status(&h, t(1, 12)),
            HealthStatus::Unknown,
            "three failures, but within a day"
        );
        observe(&mut h, t(4, 0), fail("dns", true));
        assert_eq!(host_status(&h, t(4, 0)), HealthStatus::Dead);
        assert_eq!(h.first_failed.as_deref(), Some("2026-11-02T00:00:00Z"));
        assert_eq!(h.last_ok.as_deref(), Some("2026-11-01T00:00:00Z"));

        observe(&mut h, t(5, 0), Outcome::Answered);
        assert_eq!(host_status(&h, t(5, 0)), HealthStatus::Alive);
        assert_eq!((h.failures_in_a_row, h.first_failed.as_deref()), (0, None));
    }

    #[test]
    fn no_clear_answer_never_counts_toward_dead() {
        let mut h = HostHealth::default();
        for day in 0..10 {
            observe(&mut h, t(day, 0), fail("timeout", false));
        }
        assert_eq!(host_status(&h, t(10, 0)), HealthStatus::Unknown);
        assert_eq!(h.failures_in_a_row, 0);
        assert_eq!(h.failure.as_deref(), Some("timeout"));
    }

    #[test]
    fn a_service_is_alive_if_any_host_is_and_dead_only_if_all_are() {
        let alive = || {
            let mut h = HostHealth::default();
            observe(&mut h, t(9, 0), Outcome::Answered);
            h
        };
        let dead = || {
            let mut h = HostHealth::default();
            for (d, hr) in [(0, 0), (2, 0), (4, 0)] {
                observe(&mut h, t(d, hr), fail("tls_expired", true));
            }
            h
        };
        let now = t(9, 1);
        assert_eq!(
            service_health(vec![alive(), dead()], vec![], now).status,
            HealthStatus::Alive
        );
        let d = service_health(vec![dead(), dead()], vec![], now);
        assert_eq!(d.status, HealthStatus::Dead);
        assert_eq!(d.since.as_deref(), Some("2026-11-01T00:00:00Z"));
        assert_eq!(
            service_health(vec![dead(), HostHealth::default()], vec![], now).status,
            HealthStatus::Unknown
        );
        assert_eq!(
            service_health(vec![], vec![], now).status,
            HealthStatus::Unknown
        );
    }
}
