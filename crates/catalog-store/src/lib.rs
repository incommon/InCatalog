// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Where snapshots live. One URI scheme everywhere so the same binaries run on a
//! laptop and in Lambda:
//!
//! - `./data/catalog.json.gz` or `file:///…` — local disk
//! - `s3://bucket/key` — Amazon S3 (feature `s3`, on by default)
//! - `https://…` — any static host, read-only

use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use catalog_core::model::Snapshot;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;

#[derive(Debug, Clone)]
pub enum Location {
    File(PathBuf),
    S3 { bucket: String, key: String },
    Http(String),
}

/// Bytes plus an opaque version token used to skip reloads when nothing changed.
pub struct Blob {
    pub bytes: Vec<u8>,
    pub version: String,
}

impl Location {
    pub fn parse(uri: &str) -> Result<Self> {
        if let Some(rest) = uri.strip_prefix("s3://") {
            let (bucket, key) = rest
                .split_once('/')
                .context("s3 URI must be s3://bucket/key")?;
            return Ok(Location::S3 {
                bucket: bucket.into(),
                key: key.into(),
            });
        }
        if uri.starts_with("https://") || uri.starts_with("http://") {
            return Ok(Location::Http(uri.into()));
        }
        Ok(Location::File(PathBuf::from(
            uri.strip_prefix("file://").unwrap_or(uri),
        )))
    }

    /// Returns `None` when the stored version equals `known_version`.
    pub async fn read(&self, known_version: Option<&str>) -> Result<Option<Blob>> {
        match self {
            Location::File(p) => {
                let meta = tokio::fs::metadata(p)
                    .await
                    .with_context(|| format!("reading {}", p.display()))?;
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
                let version = format!(
                    "{}-{}",
                    meta.len(),
                    mtime.map(|d| d.as_nanos()).unwrap_or(0)
                );
                if known_version == Some(version.as_str()) {
                    return Ok(None);
                }
                Ok(Some(Blob {
                    bytes: tokio::fs::read(p).await?,
                    version,
                }))
            }
            Location::Http(url) => {
                let mut req = reqwest::Client::new().get(url);
                if let Some(v) = known_version {
                    req = req.header("If-None-Match", v);
                }
                let resp = req.send().await?;
                if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
                    return Ok(None);
                }
                let resp = resp.error_for_status()?;
                let version = resp
                    .headers()
                    .get("etag")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .to_owned();
                Ok(Some(Blob {
                    bytes: resp.bytes().await?.to_vec(),
                    version,
                }))
            }
            #[cfg(feature = "s3")]
            Location::S3 { bucket, key } => {
                let client = s3_client().await;
                let head = client.head_object().bucket(bucket).key(key).send().await?;
                let version = head.e_tag().unwrap_or_default().to_owned();
                if !version.is_empty() && known_version == Some(version.as_str()) {
                    return Ok(None);
                }
                let obj = client.get_object().bucket(bucket).key(key).send().await?;
                let version = obj.e_tag().map(str::to_owned).unwrap_or(version);
                let bytes = obj.body.collect().await?.into_bytes().to_vec();
                Ok(Some(Blob { bytes, version }))
            }
            #[cfg(not(feature = "s3"))]
            Location::S3 { .. } => bail!("built without S3 support"),
        }
    }

    pub async fn write(&self, bytes: Vec<u8>, content_type: &str) -> Result<()> {
        match self {
            Location::File(p) => {
                if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
                    tokio::fs::create_dir_all(dir).await?;
                }
                // Write-then-rename so a reader never sees a half-written snapshot.
                let tmp = p.with_extension("tmp");
                tokio::fs::write(&tmp, bytes).await?;
                tokio::fs::rename(&tmp, p).await?;
                Ok(())
            }
            Location::Http(_) => bail!("HTTP locations are read-only"),
            #[cfg(feature = "s3")]
            Location::S3 { bucket, key } => {
                s3_client()
                    .await
                    .put_object()
                    .bucket(bucket)
                    .key(key)
                    .content_type(content_type)
                    .body(bytes.into())
                    .send()
                    .await?;
                Ok(())
            }
            #[cfg(not(feature = "s3"))]
            Location::S3 { .. } => {
                let _ = (bytes, content_type);
                bail!("built without S3 support")
            }
        }
    }
}

#[cfg(feature = "s3")]
async fn s3_client() -> aws_sdk_s3::Client {
    static CLIENT: tokio::sync::OnceCell<aws_sdk_s3::Client> = tokio::sync::OnceCell::const_new();
    CLIENT
        .get_or_init(|| async { aws_sdk_s3::Client::new(&aws_config::load_from_env().await) })
        .await
        .clone()
}

pub fn encode(snapshot: &Snapshot) -> Result<Vec<u8>> {
    let mut gz = GzEncoder::new(Vec::new(), Compression::best());
    serde_json::to_writer(&mut gz, snapshot)?;
    gz.flush()?;
    Ok(gz.finish()?)
}

/// Accepts gzip or plain JSON.
pub fn decode(bytes: &[u8]) -> Result<Snapshot> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut json = Vec::new();
        GzDecoder::new(bytes).read_to_end(&mut json)?;
        Ok(serde_json::from_slice(&json)?)
    } else {
        Ok(serde_json::from_slice(bytes)?)
    }
}
