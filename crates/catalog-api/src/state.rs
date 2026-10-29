// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! The loaded catalog, and how it stays current.
//!
//! Ingest publishes a new snapshot on a schedule; the API notices by checking the
//! snapshot's version token at most once per `refresh_every`, inline on a request.
//! Inline (rather than a background task) because Lambda freezes the process
//! between invocations, so a timer would not fire reliably.

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use catalog_core::search::Catalog;
use catalog_store::Location;
use tokio::sync::Mutex;

use crate::nl::ClaudeReader;

pub struct AppState {
    pub reader: Option<ClaudeReader>,
    location: Location,
    refresh_every: Duration,
    current: RwLock<Arc<Catalog>>,
    check: Mutex<(Instant, String)>,
}

impl AppState {
    pub async fn load(
        location: Location,
        refresh_every: Duration,
        reader: Option<ClaudeReader>,
    ) -> Result<Arc<Self>> {
        let blob = location
            .read(None)
            .await?
            .ok_or_else(|| anyhow::anyhow!("snapshot read returned no data"))?;
        let catalog = build(&blob.bytes).await?;
        Ok(Arc::new(Self {
            reader,
            location,
            refresh_every,
            current: RwLock::new(Arc::new(catalog)),
            check: Mutex::new((Instant::now(), blob.version)),
        }))
    }

    #[cfg(test)]
    pub fn from_catalog(catalog: Catalog) -> Arc<Self> {
        Arc::new(Self {
            reader: None,
            location: Location::Http(String::new()),
            refresh_every: Duration::MAX,
            current: RwLock::new(Arc::new(catalog)),
            check: Mutex::new((Instant::now(), String::new())),
        })
    }

    pub async fn catalog(&self) -> Arc<Catalog> {
        self.maybe_refresh().await;
        self.current.read().unwrap().clone()
    }

    async fn maybe_refresh(&self) {
        // One refresher at a time; everyone else keeps serving the current catalog.
        let Ok(mut guard) = self.check.try_lock() else {
            return;
        };
        if guard.0.elapsed() < self.refresh_every {
            return;
        }
        guard.0 = Instant::now();
        match self.location.read(Some(&guard.1)).await {
            Ok(None) => {}
            Ok(Some(blob)) => match build(&blob.bytes).await {
                Ok(c) => {
                    tracing::info!(version = %blob.version, "loaded new snapshot");
                    *self.current.write().unwrap() = Arc::new(c);
                    guard.1 = blob.version;
                }
                Err(e) => {
                    tracing::error!(error = %e, "new snapshot failed to load; keeping the old one")
                }
            },
            Err(e) => tracing::warn!(error = %e, "snapshot check failed; keeping the current one"),
        }
    }
}

async fn build(bytes: &[u8]) -> Result<Catalog> {
    let bytes = bytes.to_vec();
    tokio::task::spawn_blocking(move || Ok(Catalog::new(catalog_store::decode(&bytes)?))).await?
}
