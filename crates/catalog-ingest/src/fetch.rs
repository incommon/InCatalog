// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Downloads, with an on-disk cache and conditional GETs.
//!
//! The InCommon aggregate is ~115 MiB and eduGAIN's ~90 MiB. Both are streamed to
//! disk (never held in memory) and hashed on the way, and a re-run against an
//! unchanged aggregate costs a single 304.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use catalog_core::model::SourceInfo;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

const USER_AGENT: &str = concat!("federation-service-catalog/", env!("CARGO_PKG_VERSION"));

pub fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(600))
        .build()?)
}

pub async fn small(http: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    Ok(http
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec())
}

#[derive(Serialize, Deserialize, Default)]
struct CacheMeta {
    url: String,
    etag: Option<String>,
    last_modified: Option<String>,
    sha256: String,
    bytes: u64,
    fetched_at: String,
}

fn meta_path(file: &Path) -> PathBuf {
    file.with_extension("meta.json")
}

pub async fn aggregate(http: &reqwest::Client, url: &str, file: &Path) -> Result<SourceInfo> {
    let cached: Option<CacheMeta> = tokio::fs::read(meta_path(file))
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .filter(|m: &CacheMeta| m.url == url && file.exists());

    let mut req = http.get(url);
    if let Some(m) = &cached {
        if let Some(e) = &m.etag {
            req = req.header("If-None-Match", e);
        }
        if let Some(lm) = &m.last_modified {
            req = req.header("If-Modified-Since", lm);
        }
    }
    let mut resp = req.send().await.with_context(|| format!("GET {url}"))?;

    let meta = if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
        tracing::info!(url, "not modified; using cache");
        cached.context("304 without a cache entry")?
    } else {
        let resp_ok = resp.status().is_success();
        if !resp_ok {
            bail!("GET {url}: HTTP {}", resp.status());
        }
        let header = |n: &str| {
            resp.headers()
                .get(n)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let (etag, last_modified) = (header("etag"), header("last-modified"));
        let tmp = file.with_extension("part");
        let mut out = tokio::fs::File::create(&tmp).await?;
        let mut hasher = Sha256::new();
        let mut bytes = 0u64;
        while let Some(chunk) = resp.chunk().await? {
            hasher.update(&chunk);
            bytes += chunk.len() as u64;
            out.write_all(&chunk).await?;
        }
        out.flush().await?;
        drop(out);
        tokio::fs::rename(&tmp, file).await?;
        let m = CacheMeta {
            url: url.to_owned(),
            etag,
            last_modified,
            sha256: hex::encode(hasher.finalize()),
            bytes,
            fetched_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        };
        tokio::fs::write(meta_path(file), serde_json::to_vec_pretty(&m)?).await?;
        tracing::info!(url, bytes, "downloaded");
        m
    };

    Ok(SourceInfo {
        url: url.to_owned(),
        fetched_at: meta.fetched_at,
        last_modified: meta.last_modified,
        etag: meta.etag,
        sha256: Some(meta.sha256),
        bytes: meta.bytes,
        ..Default::default()
    })
}

/// Verify the aggregate's enveloped XML signature with the `xmlsec1` CLI against a
/// pinned federation signing certificate. Canonicalization is easy to get subtly
/// wrong, so this defers to the reference implementation rather than reimplementing it.
pub async fn verify_signature(file: &Path, cert: &Path) -> Result<()> {
    let out = tokio::process::Command::new("xmlsec1")
        .arg("--verify")
        .arg("--pubkey-cert-pem")
        .arg(cert)
        .arg("--id-attr:ID")
        .arg("urn:oasis:names:tc:SAML:2.0:metadata:EntitiesDescriptor")
        .arg(file)
        .output()
        .await
        .context("running xmlsec1 (is it installed?)")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}
