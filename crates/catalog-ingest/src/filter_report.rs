// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! InCommon's import filter report: why `edugain-only` services are out of its import.
//!
//! The newest log in the index is fetched on every run (about 25 KiB; the index
//! itself is about 2 MiB). A missing or unreadable report is a snapshot warning, not
//! a failed run: the services are still listed, only without a reason.

use anyhow::{Context, Result};
use catalog_core::import_filter;
use catalog_core::model::{FilterReportInfo, Service};
use chrono::{DateTime, SecondsFormat, Utc};

/// Older than this, the report no longer describes the aggregates being read.
const STALE_AFTER_HOURS: i64 = 48;

pub async fn attach(
    http: &reqwest::Client,
    index_url: &str,
    services: &mut [Service],
    warnings: &mut Vec<String>,
) -> Option<FilterReportInfo> {
    match fetch_and_apply(http, index_url, services).await {
        Ok(info) => {
            let age = Utc::now()
                - DateTime::parse_from_rfc3339(&info.reported_at)
                    .map(|t| t.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
            if age.num_hours() > STALE_AFTER_HOURS {
                warnings.push(format!(
                    "InCommon's newest import filter report is from {}, {} hours ago; filtered services' reasons may be out of date",
                    info.reported_at,
                    age.num_hours()
                ));
            }
            tracing::info!(?info, "import filter report");
            Some(info)
        }
        Err(e) => {
            warnings.push(format!(
                "InCommon's import filter report unavailable; filtered services are shown without a reason: {e:#}"
            ));
            None
        }
    }
}

async fn fetch_and_apply(
    http: &reqwest::Client,
    index_url: &str,
    services: &mut [Service],
) -> Result<FilterReportInfo> {
    let index = crate::fetch::small(http, index_url).await?;
    let (name, unix) = import_filter::latest_report(&String::from_utf8_lossy(&index))
        .context("the report index lists no logs")?;
    // The index links each log by name, beside itself.
    let url = reqwest::Url::parse(index_url)?.join(&name)?.to_string();
    let log = crate::fetch::small(http, &url).await?;
    let report = import_filter::parse_report(&String::from_utf8_lossy(&log));
    let reported_at = DateTime::from_timestamp(unix, 0)
        .context("log name has no valid time")?
        .to_rfc3339_opts(SecondsFormat::Secs, true);
    let applied = import_filter::apply(services, &report, &url, &reported_at);
    Ok(FilterReportInfo {
        url,
        reported_at,
        fetched_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        entities: report.len(),
        services_with_reason: applied.with_reason,
        services_without_reason: applied.without_reason,
    })
}

/// Runs against a stand-in for InCommon's report directory.
#[cfg(test)]
mod tests {
    use axum::http::{StatusCode, Uri};
    use axum::response::IntoResponse;
    use axum::Router;
    use catalog_core::model::Availability;

    use super::*;

    /// Serves `index` at `/reports/index.html` and `log` under any `.log` name.
    async fn stub(index: Option<String>, log: &'static str) -> String {
        let app = Router::new().fallback(move |uri: Uri| {
            let index = index.clone();
            async move {
                match (uri.path(), index) {
                    ("/reports/index.html", Some(i)) => i.into_response(),
                    (p, _) if p.starts_with("/reports/md_filter_report_") => log.into_response(),
                    _ => StatusCode::NOT_FOUND.into_response(),
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}/reports/index.html")
    }

    const LOG: &str = "     [java] ERROR - Item https://sp.example.org/sp (XX) was marked with the following Error status messages
     [java] ERROR -     check_sirtfi: SIRTFI requires a REFEDS security contact
";

    fn index_at(unix: i64) -> String {
        format!(
            r#"<a href="md_filter_report_1464916509.log">old</a> <a href="md_filter_report_{unix}.log">new</a>"#
        )
    }

    fn services() -> Vec<Service> {
        ["https://sp.example.org/sp", "https://other.example.org/sp"]
            .map(|id| Service {
                entity_id: id.into(),
                availability: Some(Availability::EdugainOnly),
                ..Default::default()
            })
            .to_vec()
    }

    #[tokio::test]
    async fn attaches_the_newest_report() {
        let now = Utc::now().timestamp();
        let index_url = stub(Some(index_at(now)), LOG).await;
        let (mut services, mut warnings) = (services(), Vec::new());
        let http = crate::fetch::client().unwrap();
        let info = attach(&http, &index_url, &mut services, &mut warnings)
            .await
            .unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(info
            .url
            .ends_with(&format!("/reports/md_filter_report_{now}.log")));
        assert_eq!(
            (
                info.entities,
                info.services_with_reason,
                info.services_without_reason
            ),
            (1, 1, 1)
        );
        let f = services[0].import_filter.as_ref().unwrap();
        assert_eq!(f.findings[0].reason, "sirtfi");
        assert_eq!(f.report_url, info.url);
        assert!(services[1].import_filter.is_none());
    }

    #[tokio::test]
    async fn warns_about_a_stale_report() {
        let three_days_ago = Utc::now().timestamp() - 3 * 24 * 3600;
        let index_url = stub(Some(index_at(three_days_ago)), LOG).await;
        let (mut services, mut warnings) = (services(), Vec::new());
        let http = crate::fetch::client().unwrap();
        assert!(attach(&http, &index_url, &mut services, &mut warnings)
            .await
            .is_some());
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("may be out of date"),
            "{}",
            warnings[0]
        );
    }

    #[tokio::test]
    async fn a_missing_report_is_a_warning() {
        let http = crate::fetch::client().unwrap();
        for index in [None, Some("<html>no logs today</html>".to_owned())] {
            let index_url = stub(index, LOG).await;
            let (mut services, mut warnings) = (services(), Vec::new());
            assert!(attach(&http, &index_url, &mut services, &mut warnings)
                .await
                .is_none());
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].contains("unavailable"), "{}", warnings[0]);
            assert!(services.iter().all(|s| s.import_filter.is_none()));
        }
    }
}
