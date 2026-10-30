// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! A run's time budget.
//!
//! Lambda kills a run that reaches the function's timeout, before it saves any cache
//! or writes the snapshot, so the next run repeats the same work and can be killed
//! again. Each step that calls out (Claude, websites) therefore stops starting new
//! work at its deadline, keeps what it has, and lets the run finish. What it did not
//! start waits for the next run, as work over a step's per-run cap already does.

use std::future::ready;
use std::time::{Duration, Instant, SystemTime};

use futures::{Stream, StreamExt};

/// Time kept back after the last step's deadline: for requests already in flight (a
/// website lookup with its retries can take about a minute), saving the caches, and
/// building and writing the snapshot.
pub const RESERVE: Duration = Duration::from_secs(120);

/// When to stop starting new work. `None` is no limit: a local run, unless
/// `--time-limit-secs` sets one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Deadline(Option<Instant>);

impl Deadline {
    pub fn none() -> Self {
        Self(None)
    }

    /// `left` from now, less [`RESERVE`].
    pub fn after(left: Duration) -> Self {
        Self(Some(Instant::now() + left.saturating_sub(RESERVE)))
    }

    /// The Lambda invocation's deadline, less [`RESERVE`].
    pub fn lambda(deadline: SystemTime) -> Self {
        Self::after(
            deadline
                .duration_since(SystemTime::now())
                .unwrap_or_default(),
        )
    }

    pub fn passed(&self) -> bool {
        self.0.is_some_and(|d| Instant::now() >= d)
    }

    /// One step's deadline: `share` of the time left, so the steps after it keep the
    /// rest. The last step takes the whole of what is left.
    pub fn share(&self, share: f64) -> Self {
        Self(self.0.map(|d| {
            let now = Instant::now();
            now + d
                .saturating_duration_since(now)
                .mul_f64(share.clamp(0.0, 1.0))
        }))
    }

    /// Stop taking items from `s` once the deadline has passed. Items already taken
    /// still run to completion.
    pub fn until<S: Stream>(self, s: S) -> impl Stream<Item = S::Item> {
        s.take_while(move |_| ready(!self.passed()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    #[test]
    fn no_limit_never_passes() {
        let d = Deadline::none();
        assert!(!d.passed());
        assert_eq!(d.share(0.5), d);
        assert_eq!(Deadline::default(), d);
    }

    #[test]
    fn the_reserve_comes_off_the_top() {
        // Less time than the reserve leaves nothing to start.
        assert!(Deadline::after(RESERVE / 2).passed());
        assert!(Deadline::lambda(SystemTime::now() + RESERVE / 2).passed());
        assert!(Deadline::lambda(SystemTime::now() - Duration::from_secs(1)).passed());
        let d = Deadline::lambda(SystemTime::now() + RESERVE + Duration::from_secs(60));
        assert!(!d.passed());
        let left = d.0.unwrap() - Instant::now();
        assert!(left > Duration::from_secs(55) && left <= Duration::from_secs(60));
    }

    #[test]
    fn a_share_leaves_the_rest_for_later_steps() {
        let whole = Deadline::after(RESERVE + Duration::from_secs(100));
        let half = whole.share(0.5).0.unwrap() - Instant::now();
        assert!(half > Duration::from_secs(45) && half <= Duration::from_secs(50));
        assert_eq!(whole.share(1.0).0.unwrap(), whole.0.unwrap());
        assert!(whole.share(0.0).passed());
        assert!(whole.share(-1.0).passed(), "a share below zero is none");
    }

    #[tokio::test]
    async fn until_stops_taking_work() {
        let all: Vec<u32> = Deadline::none().until(stream::iter(0..5)).collect().await;
        assert_eq!(all, [0, 1, 2, 3, 4]);
        let none: Vec<u32> = Deadline::after(Duration::ZERO)
            .until(stream::iter(0..5))
            .collect()
            .await;
        assert!(none.is_empty());
    }
}
