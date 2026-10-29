// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Build a catalog snapshot from live federation metadata.
//!
//! Runs as a CLI locally and as a scheduled Lambda when `AWS_LAMBDA_RUNTIME_API` is
//! set; configuration comes from flags or the matching `CATALOG_*` environment
//! variables in both cases.

mod enrich;
mod fetch;
mod logos;
mod sector;
mod sharing;

use std::io::BufReader;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use catalog_core::build::Builder;
use catalog_core::curation::CurationFile;
use catalog_core::federations::{parse_edugain_federations, EDUGAIN_FEDERATIONS_URL};
use catalog_core::model::{Snapshot, Source};
use catalog_core::SNAPSHOT_SCHEMA_VERSION;
use catalog_store::Location;
use clap::Parser;

pub const INCOMMON_AGGREGATE_URL: &str = "https://mdq.incommon.org/entities";
pub const EDUGAIN_AGGREGATE_URL: &str = "https://mds.edugain.org/edugain-v2.xml";

#[derive(Parser, Debug, Clone)]
#[command(version, about)]
struct Args {
    /// Where to write the snapshot: a path, s3://bucket/key.
    #[arg(long, env = "CATALOG_OUT", default_value = "data/catalog.json.gz")]
    out: String,

    /// Directory for downloaded aggregates; re-runs use conditional GETs.
    #[arg(long, env = "CATALOG_CACHE_DIR")]
    cache_dir: Option<PathBuf>,

    #[arg(long, env = "CATALOG_INCOMMON_URL", default_value = INCOMMON_AGGREGATE_URL)]
    incommon_url: String,

    #[arg(long, env = "CATALOG_EDUGAIN_URL", default_value = EDUGAIN_AGGREGATE_URL)]
    edugain_url: String,

    #[arg(long, env = "CATALOG_FEDERATIONS_URL", default_value = EDUGAIN_FEDERATIONS_URL)]
    federations_url: String,

    /// Skip the eduGAIN aggregate (InCommon view only).
    #[arg(long, env = "CATALOG_SKIP_EDUGAIN")]
    skip_edugain: bool,

    /// Optional editorial overlay (JSON), as a path, s3:// or https:// URI.
    #[arg(long, env = "CATALOG_CURATION")]
    curation: Option<String>,

    /// Verify aggregate signatures with `xmlsec1` against this InCommon signing cert (PEM).
    #[arg(long, env = "CATALOG_INCOMMON_CERT")]
    incommon_cert: Option<PathBuf>,

    /// Verify the eduGAIN aggregate signature against this cert (PEM).
    #[arg(long, env = "CATALOG_EDUGAIN_CERT")]
    edugain_cert: Option<PathBuf>,

    /// Publish even if an aggregate's validUntil has passed.
    #[arg(long, env = "CATALOG_ALLOW_EXPIRED")]
    allow_expired: bool,

    /// Skip logo checks and website lookups.
    #[arg(long, env = "CATALOG_SKIP_LOGOS")]
    skip_logos: bool,

    /// Where the logo cache lives (default: `logo-cache.json` beside `--out`).
    #[arg(long, env = "CATALOG_LOGO_CACHE")]
    logo_cache: Option<String>,

    /// Most metadata logo URLs to check per run.
    #[arg(long, env = "CATALOG_LOGO_MAX_CHECKS", default_value_t = 4000)]
    logo_max_checks: usize,

    /// Most websites to look up per run (each is one page fetch and one Claude call).
    #[arg(long, env = "CATALOG_LOGO_MAX_LOOKUPS", default_value_t = 300)]
    logo_max_lookups: usize,

    /// Generated summaries and keywords: `auto` (batch, or at once when 25 or fewer
    /// need writing), `batch`, `sync`, or `off`.
    #[arg(long, env = "CATALOG_ENRICH_MODE", default_value = "auto")]
    enrich_mode: String,

    /// Most entries to send to Claude per run.
    #[arg(long, env = "CATALOG_ENRICH_MAX", default_value_t = 10_000)]
    enrich_max: usize,

    /// Where generated summaries are kept (default: `summary-cache.json` beside `--out`).
    #[arg(long, env = "CATALOG_ENRICH_CACHE")]
    enrich_cache: Option<String>,

    /// Collect this Message Batches ID as if this cache had submitted it: for a batch
    /// whose submit reached the API but whose reply never came back.
    #[arg(long, env = "CATALOG_ENRICH_ADOPT_BATCH")]
    enrich_adopt_batch: Option<String>,

    /// Most multilateral/bilateral requests to send to Claude per run (a tenant
    /// cluster is one); 0 turns them off.
    #[arg(long, env = "CATALOG_SHARING_MAX", default_value_t = 400)]
    sharing_max: usize,

    /// Where sharing labels are kept (default: `sharing-cache.json` beside `--out`).
    #[arg(long, env = "CATALOG_SHARING_CACHE")]
    sharing_cache: Option<String>,

    /// Most organizations to ask Claude for a sector per run; 0 turns it off.
    #[arg(long, env = "CATALOG_SECTOR_MAX", default_value_t = 400)]
    sector_max: usize,

    /// Where sector labels are kept (default: `sector-cache.json` beside `--out`).
    #[arg(long, env = "CATALOG_SECTOR_CACHE")]
    sector_cache: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    if std::env::var_os("AWS_LAMBDA_RUNTIME_API").is_some() {
        // Scheduled invocation: the event payload carries nothing we need.
        let args = Args::parse_from(["catalog-ingest"]);
        return lambda_runtime::run(lambda_runtime::service_fn(
            move |_: lambda_runtime::LambdaEvent<serde_json::Value>| {
                let args = args.clone();
                async move {
                    let summary = run(args).await.map_err(|e| format!("{e:#}"))?;
                    Ok::<_, lambda_runtime::Error>(summary)
                }
            },
        ))
        .await
        .map_err(|e| anyhow::anyhow!(e));
    }

    let summary = run(Args::parse()).await?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

async fn run(args: Args) -> Result<serde_json::Value> {
    let cache = args
        .cache_dir
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("federation-service-catalog"));
    tokio::fs::create_dir_all(&cache).await?;
    let http = fetch::client()?;
    let mut warnings = Vec::new();

    let federations = match fetch::small(&http, &args.federations_url).await {
        Ok(b) => parse_edugain_federations(&b).context("parsing eduGAIN federation list")?,
        Err(e) => {
            warnings.push(format!(
                "eduGAIN federation list unavailable; registrars are shown unnamed: {e:#}"
            ));
            Vec::new()
        }
    };

    let curation = match &args.curation {
        Some(uri) => {
            let blob = Location::parse(uri)?
                .read(None)
                .await?
                .context("curation file missing")?;
            Some(CurationFile::from_json(&blob.bytes).context("parsing curation file")?)
        }
        None => None,
    };

    let mut sources: Vec<(Source, &str, Option<&PathBuf>)> = vec![(
        Source::Incommon,
        &args.incommon_url,
        args.incommon_cert.as_ref(),
    )];
    if !args.skip_edugain {
        sources.push((
            Source::Edugain,
            &args.edugain_url,
            args.edugain_cert.as_ref(),
        ));
    }

    let mut builder = Builder::new();
    let mut infos = Vec::new();
    for (source, url, cert) in sources {
        let file = cache.join(format!("{source:?}.xml").to_lowercase());
        let mut info = fetch::aggregate(&http, url, &file).await?;
        if let Some(cert) = cert {
            fetch::verify_signature(&file, cert)
                .await
                .with_context(|| format!("signature verification failed for {url}"))?;
            info.signature_verified = true;
        }
        let path = file.clone();
        let (b, info) = tokio::task::spawn_blocking(move || -> Result<_> {
            let reader = BufReader::with_capacity(1 << 20, std::fs::File::open(&path)?);
            builder.add_aggregate(source, reader, &mut info)?;
            Ok((builder, info))
        })
        .await??;
        builder = b;
        check_valid_until(&info.valid_until, url, args.allow_expired, &mut warnings)?;
        tracing::info!(
            ?source,
            entities = info.entities,
            sps = info.service_providers,
            "parsed"
        );
        infos.push(info);
    }

    let (mut services, idp_counts) = builder.finish(&federations, curation.as_ref());

    let enrich_mode = enrich::Mode::parse(&args.enrich_mode);
    let enrich_report = if enrich_mode == enrich::Mode::Off {
        None
    } else {
        let cache = args
            .enrich_cache
            .clone()
            .unwrap_or_else(|| sibling(&args.out, "summary-cache.json"));
        let claude = catalog_claude::Claude::from_env(
            "CATALOG_ENRICH_MODEL",
            "CATALOG_ENRICH_EFFORT",
            std::time::Duration::from_secs(60),
        )
        .await?;
        let report = enrich::enrich(
            &mut services,
            &federations,
            enrich::Options {
                cache: Location::parse(&cache)?,
                claude,
                mode: enrich_mode,
                max: args.enrich_max,
                adopt_batch: args.enrich_adopt_batch.clone(),
            },
        )
        .await?;
        tracing::info!(?report, "summaries");
        if let Some(why) = &report.skipped_reason {
            warnings.push(format!(
                "{} services are waiting for a generated summary: {why}",
                report.waiting
            ));
        }
        if let Some(e) = &report.batch_error {
            warnings.push(format!("summary batch: {e}"));
        }
        Some(report)
    };

    let sharing_report = if args.sharing_max == 0 {
        None
    } else {
        let cache = args
            .sharing_cache
            .clone()
            .unwrap_or_else(|| sibling(&args.out, "sharing-cache.json"));
        let claude = catalog_claude::Claude::from_env(
            "CATALOG_SHARING_MODEL",
            "CATALOG_SHARING_EFFORT",
            std::time::Duration::from_secs(60),
        )
        .await?;
        let report = sharing::classify(
            &mut services,
            &federations,
            sharing::Options {
                cache: Location::parse(&cache)?,
                claude,
                max: args.sharing_max,
            },
        )
        .await?;
        tracing::info!(?report, "sharing");
        if let Some(why) = &report.skipped_reason {
            warnings.push(format!(
                "{} multilateral/bilateral requests are waiting: {why}",
                report.waiting
            ));
        }
        Some(report)
    };

    let sector_report = if args.sector_max == 0 {
        None
    } else {
        let cache = args
            .sector_cache
            .clone()
            .unwrap_or_else(|| sibling(&args.out, "sector-cache.json"));
        let claude = catalog_claude::Claude::from_env(
            "CATALOG_SECTOR_MODEL",
            "CATALOG_SECTOR_EFFORT",
            std::time::Duration::from_secs(60),
        )
        .await?;
        let report = sector::classify(
            &mut services,
            &federations,
            sector::Options {
                cache: Location::parse(&cache)?,
                claude,
                max: args.sector_max,
            },
        )
        .await?;
        tracing::info!(?report, "sectors");
        if let Some(why) = &report.skipped_reason {
            warnings.push(format!(
                "{} organizations are waiting for a sector: {why}",
                report.waiting
            ));
        }
        Some(report)
    };

    let logo_report = if args.skip_logos {
        None
    } else {
        let cache = args
            .logo_cache
            .clone()
            .unwrap_or_else(|| sibling(&args.out, "logo-cache.json"));
        // Choosing among a page's candidate images is a small, well-specified task.
        let claude = catalog_claude::Claude::from_env(
            "CATALOG_LOGO_MODEL",
            "CATALOG_LOGO_EFFORT",
            std::time::Duration::from_secs(60),
        )
        .await?;
        let report = logos::enrich(
            &mut services,
            logos::Options {
                cache: Location::parse(&cache)?,
                max_checks: args.logo_max_checks,
                max_lookups: args.logo_max_lookups,
                claude,
            },
        )
        .await?;
        tracing::info!(?report, "logos");
        if report.sites_skipped_network_down > 0 {
            warnings.push(format!(
                "website logo lookups stopped after repeated connection failures; {} sites wait for the next run",
                report.sites_skipped_network_down
            ));
        }
        if let Some(why) = &report.lookup_skipped_reason {
            warnings.push(format!(
                "{} services have no working logo; website lookup skipped: {why}",
                report.services_without_working_logo
            ));
        }
        Some(report)
    };
    let snapshot = Snapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        generated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        generator: format!("catalog-ingest {}", env!("CARGO_PKG_VERSION")),
        sources: infos,
        federations,
        services,
        idp_counts,
        warnings,
    };

    let bytes = catalog_store::encode(&snapshot)?;
    let size = bytes.len();
    Location::parse(&args.out)?
        .write(bytes, "application/json")
        .await?;
    tracing::info!(out = %args.out, bytes = size, services = snapshot.services.len(), "snapshot written");

    Ok(serde_json::json!({
        "out": args.out,
        "bytes": size,
        "services": snapshot.services.len(),
        "federations": snapshot.federations.len(),
        "sources": snapshot.sources,
        "warnings": snapshot.warnings,
        "logos": logo_report,
        "summaries": enrich_report,
        "sharing": sharing_report,
        "sectors": sector_report,
    }))
}

/// `data/catalog.json.gz` → `data/<name>`; `s3://b/k/catalog.json.gz` → `s3://b/k/<name>`.
fn sibling(out: &str, name: &str) -> String {
    match out.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/{name}"),
        None => name.to_owned(),
    }
}

/// An expired aggregate is one a conforming SAML consumer would refuse; publishing a
/// catalog built from it would advertise stale state as current.
fn check_valid_until(
    valid_until: &Option<String>,
    url: &str,
    allow: bool,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let Some(v) = valid_until else {
        warnings.push(format!("{url} has no validUntil"));
        return Ok(());
    };
    let t =
        chrono::DateTime::parse_from_rfc3339(v).with_context(|| format!("bad validUntil {v}"))?;
    if t < chrono::Utc::now() {
        let msg = format!("{url} expired at {v}");
        if !allow {
            bail!("{msg}; refusing to publish (use --allow-expired to override)");
        }
        warnings.push(msg);
    }
    Ok(())
}
