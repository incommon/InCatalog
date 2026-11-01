// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! OFAC's sanctions lists: which services' organizations may be on them.
//!
//! Both lists are fetched on every run beside the aggregates and streamed to the
//! cache directory (the SDN list is about 30 MiB; on a warm cache an unchanged list
//! costs a 304). If either list can't be read, the snapshot carries a warning and no
//! service has a match, rather than matches against half of OFAC's lists.
//!
//! The step runs before the steps that share the run's time budget, so it has a
//! fixed limit of its own ([`TIME_LIMIT`]): a slow download can't use up the
//! Lambda's 15 minutes before any cache is saved or the snapshot is written.

use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use catalog_core::model::{OfacListInfo, OfacReport, Service};
use catalog_core::ofac::{self, Decision, Index, List};

/// Both lists are usually read in a few seconds, about 30 MiB in all.
pub const TIME_LIMIT: Duration = Duration::from_secs(90);

pub struct Options<'a> {
    pub sdn: &'a str,
    pub consolidated: &'a str,
    pub cache: &'a Path,
    /// The whole step, both lists; [`TIME_LIMIT`] outside tests.
    pub time_limit: Duration,
}

pub async fn attach(
    http: &reqwest::Client,
    opts: Options<'_>,
    services: &mut [Service],
    decisions: &[Decision],
    warnings: &mut Vec<String>,
) -> Option<OfacReport> {
    let unavailable = |what: String| format!("{what}; services are shown without OFAC matches");
    let (index, lists) = match tokio::time::timeout(opts.time_limit, read_all(http, &opts)).await {
        Ok(Ok(read)) => read,
        Ok(Err((list, e))) => {
            warnings.push(unavailable(format!("{} unavailable: {e:#}", list.label())));
            return None;
        }
        Err(_) => {
            warnings.push(unavailable(format!(
                "OFAC's lists took longer than {} s to read",
                opts.time_limit.as_secs()
            )));
            return None;
        }
    };
    let applied = ofac::apply(services, &index, decisions);
    for d in &applied.unused_decisions {
        warnings.push(format!(
            "the curation overlay's OFAC decision on {d} matched nothing: the entry may have left OFAC's lists, or the service or its name changed"
        ));
    }
    let report = OfacReport {
        lists,
        possible: applied.possible,
        confirmed: applied.confirmed,
        dismissed: applied.dismissed,
        providers: applied.providers,
    };
    tracing::info!(?report, "OFAC lists");
    Some(report)
}

/// Both lists, or the first that failed. Nothing is attached until both are read.
async fn read_all(
    http: &reqwest::Client,
    opts: &Options<'_>,
) -> std::result::Result<(Index, Vec<OfacListInfo>), (List, anyhow::Error)> {
    let mut index = Index::default();
    let mut lists = Vec::new();
    for (list, url) in [
        (List::Sdn, opts.sdn),
        (List::Consolidated, opts.consolidated),
    ] {
        let (parsed, info) = read(http, list, url, opts.cache)
            .await
            .map_err(|e| (list, e))?;
        index.add(list, parsed);
        lists.push(info);
    }
    Ok((index, lists))
}

async fn read(
    http: &reqwest::Client,
    list: List,
    url: &str,
    cache: &Path,
) -> Result<(ofac::Parsed, OfacListInfo)> {
    let file = cache.join(format!("ofac-{}.xml", list.key()));
    let source = crate::fetch::aggregate(http, url, &file).await?;
    let parsed = tokio::task::spawn_blocking(move || -> Result<_> {
        let reader = BufReader::with_capacity(1 << 20, std::fs::File::open(&file)?);
        ofac::parse_list(reader)
    })
    .await?
    .with_context(|| format!("parsing {url}"))?;
    anyhow::ensure!(
        parsed.published.is_some(),
        "{url} has no publication date: not one of OFAC's list files"
    );
    let info = OfacListInfo {
        list: list.key().to_owned(),
        url: url.to_owned(),
        published: parsed.published.clone(),
        fetched_at: source.fetched_at,
        entities: parsed.entities.len(),
    };
    Ok((parsed, info))
}

/// Runs against a stand-in for OFAC's Sanctions List Service.
#[cfg(test)]
mod tests {
    use axum::http::{StatusCode, Uri};
    use axum::response::IntoResponse;
    use axum::Router;

    use super::*;

    const SDN: &str = r#"<sdnList xmlns="https://sanctionslistservice.ofac.treas.gov/api/PublicationPreview/exports/XML">
  <publshInformation><Publish_Date>10/28/2026</Publish_Date></publshInformation>
  <sdnEntry><uid>306</uid><lastName>BANCO NACIONAL DE CUBA</lastName><sdnType>Entity</sdnType>
    <programList><program>CUBA</program></programList>
    <idList><id><uid>7</uid><idType>Website</idType><idNumber>www.bnc.cu</idNumber></id></idList>
  </sdnEntry>
  <sdnEntry><uid>9</uid><firstName>Ana</firstName><lastName>EXAMPLE RESEARCH</lastName><sdnType>Individual</sdnType></sdnEntry>
</sdnList>"#;

    const CONSOLIDATED: &str = r#"<sdnList>
  <publshInformation><Publish_Date>10/19/2026</Publish_Date></publshInformation>
  <sdnEntry><uid>30941</uid><lastName>EXAMPLE TELECOM GROUP CO., LTD.</lastName><sdnType>Entity</sdnType>
    <programList><program>CMIC-EO13959</program></programList>
  </sdnEntry>
</sdnList>"#;

    /// Serves the SDN list at `/SDN.XML` (or a 404 when `sdn` is false) and the
    /// consolidated list at `/CONSOLIDATED.XML`, and never finishes answering `/slow`.
    async fn stub(sdn: bool) -> String {
        let app = Router::new().fallback(move |uri: Uri| async move {
            match uri.path() {
                "/SDN.XML" if sdn => SDN.into_response(),
                "/CONSOLIDATED.XML" => CONSOLIDATED.into_response(),
                "/slow" => {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    SDN.into_response()
                }
                "/README" => "not a list".into_response(),
                _ => StatusCode::NOT_FOUND.into_response(),
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }

    fn services() -> Vec<Service> {
        let svc = |id: &str, org: &str| {
            let mut s = Service {
                entity_id: id.into(),
                ..Default::default()
            };
            s.organization.display_names.insert("en".into(), org.into());
            s
        };
        vec![
            svc("https://sso.bnc.cu/sp", "Banco Nacional"),
            svc("https://sp.example.cn/sp", "Example Telecom Group Co.Ltd"),
            svc("https://sp.example.edu/sp", "Example Research"),
        ]
    }

    fn cache() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "incatalog-ofac-test-{}-{}",
            std::process::id(),
            rand_suffix()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[tokio::test]
    async fn matches_both_lists_and_reports_them() {
        let base = stub(true).await;
        let (sdn, cons) = (
            format!("{base}/SDN.XML"),
            format!("{base}/CONSOLIDATED.XML"),
        );
        let (mut services, mut warnings) = (services(), Vec::new());
        let decisions: Vec<Decision> = serde_json::from_str(
            r#"[{"entry": "sdn:306", "provider": "Banco Nacional", "decision": "confirmed", "date": "2026-11-01"},
                {"entry": "sdn:1", "service": "https://gone.example/sp", "decision": "dismissed", "date": "2026-10-01"}]"#,
        )
        .unwrap();
        let http = crate::fetch::client().unwrap();
        let report = attach(
            &http,
            Options {
                sdn: &sdn,
                consolidated: &cons,
                cache: &cache(),
                time_limit: TIME_LIMIT,
            },
            &mut services,
            &decisions,
            &mut warnings,
        )
        .await
        .unwrap();
        assert_eq!(
            (report.confirmed, report.possible, report.providers),
            (1, 1, 2)
        );
        assert_eq!(report.lists.len(), 2);
        assert_eq!(report.lists[0].published.as_deref(), Some("2026-10-28"));
        assert_eq!(report.lists[1].entities, 1);
        // Confirmed on its domain by curation; possible on its name; an individual's
        // name never matches.
        assert_eq!(services[0].ofac[0].status, ofac::MatchStatus::Confirmed);
        assert_eq!(services[1].ofac[0].status, ofac::MatchStatus::Possible);
        assert!(services[2].ofac.is_empty());
        // The decision on an entry that matched nothing is a warning.
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("sdn:1"), "{}", warnings[0]);
    }

    #[tokio::test]
    async fn one_missing_list_means_no_matches_and_a_warning() {
        let base = stub(false).await;
        for sdn in [format!("{base}/SDN.XML"), format!("{base}/README")] {
            let cons = format!("{base}/CONSOLIDATED.XML");
            let (mut services, mut warnings) = (services(), Vec::new());
            let http = crate::fetch::client().unwrap();
            let report = attach(
                &http,
                Options {
                    sdn: &sdn,
                    consolidated: &cons,
                    cache: &cache(),
                    time_limit: TIME_LIMIT,
                },
                &mut services,
                &[],
                &mut warnings,
            )
            .await;
            assert!(report.is_none(), "{sdn}");
            assert!(services.iter().all(|s| s.ofac.is_empty()));
            assert_eq!(warnings.len(), 1);
            assert!(
                warnings[0].contains("SDN list unavailable"),
                "{}",
                warnings[0]
            );
        }
    }

    /// A list that never finishes downloading must not hold up the run.
    #[tokio::test]
    async fn a_slow_list_stops_at_the_time_limit() {
        let base = stub(true).await;
        let (sdn, cons) = (format!("{base}/slow"), format!("{base}/CONSOLIDATED.XML"));
        let (mut services, mut warnings) = (services(), Vec::new());
        let http = crate::fetch::client().unwrap();
        let started = std::time::Instant::now();
        let report = attach(
            &http,
            Options {
                sdn: &sdn,
                consolidated: &cons,
                cache: &cache(),
                time_limit: Duration::from_millis(300),
            },
            &mut services,
            &[],
            &mut warnings,
        )
        .await;
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(report.is_none());
        assert!(services.iter().all(|s| s.ofac.is_empty()));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("took longer than"), "{}", warnings[0]);
    }
}
