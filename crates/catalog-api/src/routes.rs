// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use axum::extract::{Path, Query, RawQuery, State};
use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use catalog_core::ask::AskResult;
use catalog_core::nl::{Interpretation, InterpretedParams, MatchedTerm};
use catalog_core::search::{
    AttributeView, CategoryReport, CategoryView, Counts, FacetValue, FederationView, Links, Meta,
    Provider, ProviderDetail, ProviderPage, ProviderQuery, ProviderRef, RawTag, ServiceDetail,
    ServicePage, ServiceQuery, ServiceSummary,
};
use serde::{Deserialize, Serialize};
use tower_http::set_header::SetResponseHeaderLayer;
use utoipa::{IntoParams, OpenApi, ToSchema};

use crate::state::AppState;

type St = State<Arc<AppState>>;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Federation Service Catalog API",
        description = "Services (SAML service providers) available through InCommon and eduGAIN, derived from published federation metadata."
    ),
    paths(
        meta,
        ask,
        interpret,
        services,
        service,
        lookup,
        providers,
        provider,
        federations,
        categories,
        attributes
    ),
    components(schemas(
        Meta,
        Counts,
        ServicePage,
        ServiceSummary,
        FacetValue,
        ServiceDetail,
        Links,
        ProviderRef,
        ProviderPage,
        Provider,
        ProviderDetail,
        FederationView,
        CategoryReport,
        CategoryView,
        RawTag,
        AttributeView,
        ApiError,
        Interpretation,
        InterpretedParams,
        MatchedTerm,
        AskResult,
        catalog_core::model::Service,
        catalog_core::model::Federation,
        catalog_core::model::SourceInfo,
    ))
)]
pub struct ApiDoc;

pub fn router(state: Arc<AppState>) -> Router {
    let v1 = Router::new()
        .route("/meta", get(meta))
        .route("/interpret", get(interpret))
        .route("/ask", get(ask))
        .route("/services", get(services))
        .route("/services/{id}", get(service))
        .route("/lookup", get(lookup))
        .route("/providers", get(providers))
        .route("/providers/{id}", get(provider))
        .route("/federations", get(federations))
        .route("/categories", get(categories))
        .route("/attributes", get(attributes))
        // Metadata is republished daily; five minutes of caching is plenty fresh.
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=300"),
        ));
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/openapi.json", get(|| async { Json(ApiDoc::openapi()) }))
        .nest("/v1", v1)
        .with_state(state)
}

#[derive(Serialize, ToSchema)]
pub struct ApiError {
    error: String,
}

fn not_found(what: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error: format!("{what} not found"),
        }),
    )
        .into_response()
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LangParam {
    lang: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LookupParams {
    /// The SAML entityID, exactly as published.
    entity_id: String,
}

/// Snapshot provenance, headline counts, and caveats for interpreting the data.
#[utoipa::path(get, path = "/v1/meta", responses((status = 200, body = Meta)))]
async fn meta(State(s): St) -> Json<Meta> {
    Json(s.catalog().await.meta())
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct InterpretParams {
    /// A search in plain language, e.g. "R&S services in Sweden that request email".
    q: String,
    /// `auto` (default: Claude for queries of three or more words when configured,
    /// otherwise rules), `rules`, or `claude`.
    engine: Option<String>,
}

/// Read a plain-language search into `/v1/services` filters. Pass the returned
/// `params` to `/v1/services` (with `match=any` when `q` has several words).
#[utoipa::path(get, path = "/v1/interpret", params(InterpretParams), responses((status = 200, body = Interpretation)))]
async fn interpret(State(s): St, Query(p): Query<InterpretParams>) -> Response {
    let Some(q) = valid_query(&p.q) else {
        return bad_query();
    };
    let catalog = s.catalog().await;
    let reading = read(&s, &catalog, q, &p).await;
    // Readings depend on the model, so do not let shared caches hold them long.
    (
        [(header::CACHE_CONTROL, "private, max-age=60")],
        Json(reading),
    )
        .into_response()
}

/// Read a plain-language search and find results for it, loosening the reading
/// step by step when it matches nothing (see `relaxed`). Pass the returned
/// `params` and `match` to `/v1/services` for the page of results.
#[utoipa::path(get, path = "/v1/ask", params(InterpretParams), responses((status = 200, body = AskResult)))]
async fn ask(State(s): St, Query(p): Query<InterpretParams>) -> Response {
    let Some(q) = valid_query(&p.q) else {
        return bad_query();
    };
    let catalog = s.catalog().await;
    let reading = read(&s, &catalog, q, &p).await;
    let result = catalog.ask(reading);
    (
        [(header::CACHE_CONTROL, "private, max-age=60")],
        Json(result),
    )
        .into_response()
}

fn valid_query(q: &str) -> Option<&str> {
    let q = q.trim();
    (!q.is_empty() && q.chars().count() <= 500).then_some(q)
}

fn bad_query() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiError {
            error: "q must be 1 to 500 characters".into(),
        }),
    )
        .into_response()
}

async fn read(
    s: &AppState,
    catalog: &catalog_core::search::Catalog,
    q: &str,
    p: &InterpretParams,
) -> Interpretation {
    crate::nl::interpret(
        s.reader.as_ref(),
        catalog,
        q,
        p.engine.as_deref().unwrap_or("auto"),
    )
    .await
}

/// Search services with filters, paging, and facet counts. Filters combine with
/// AND; several `federation` values combine with OR.
#[utoipa::path(get, path = "/v1/services", params(ServiceQuery), responses((status = 200, body = ServicePage)))]
async fn services(State(s): St, RawQuery(raw): RawQuery) -> Response {
    let query = join_repeated(raw.as_deref().unwrap_or(""), "federation");
    // It came from a valid URI and only gained commas, so it parses again.
    let Ok(uri) = format!("/?{query}").parse::<Uri>() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match Query::<ServiceQuery>::try_from_uri(&uri) {
        Ok(Query(q)) => Json(s.catalog().await.search(&q)).into_response(),
        Err(e) => e.into_response(),
    }
}

/// `federation=A&federation=B` → `federation=A,B`. The query deserializer takes one
/// value per field, and `ServiceQuery` reads a comma-separated list.
fn join_repeated(query: &str, key: &str) -> String {
    let (mine, rest): (Vec<&str>, Vec<&str>) = query
        .split('&')
        .filter(|p| !p.is_empty())
        .partition(|p| p.split_once('=').map_or(*p, |(k, _)| k) == key);
    if mine.len() < 2 {
        return query.to_owned();
    }
    let values: Vec<&str> = mine
        .iter()
        .filter_map(|p| p.split_once('=').map(|(_, v)| v))
        .collect();
    let mut out = rest;
    let joined = format!("{key}={}", values.join(","));
    out.push(&joined);
    out.join("&")
}

/// One service by catalog ID (the SHA-1 of its entityID; a `{sha1}` prefix is accepted).
#[utoipa::path(get, path = "/v1/services/{id}",
    params(("id" = String, Path, description = "Lowercase hex SHA-1 of the entityID")),
    responses((status = 200, body = ServiceDetail), (status = 404, body = ApiError)))]
async fn service(State(s): St, Path(id): Path<String>) -> Response {
    match s.catalog().await.service_by_id(&id) {
        Some(d) => Json(d).into_response(),
        None => not_found("service"),
    }
}

/// One service by entityID.
#[utoipa::path(get, path = "/v1/lookup", params(LookupParams),
    responses((status = 200, body = ServiceDetail), (status = 404, body = ApiError)))]
async fn lookup(State(s): St, Query(p): Query<LookupParams>) -> Response {
    match s.catalog().await.service_by_entity_id(p.entity_id.trim()) {
        Some(d) => Json(d).into_response(),
        None => not_found("service"),
    }
}

/// Organizations grouped by the name they registered under, with service counts.
#[utoipa::path(get, path = "/v1/providers", params(ProviderQuery), responses((status = 200, body = ProviderPage)))]
async fn providers(State(s): St, Query(q): Query<ProviderQuery>) -> Json<ProviderPage> {
    Json(s.catalog().await.providers(&q))
}

/// One provider and all of its services.
#[utoipa::path(get, path = "/v1/providers/{id}",
    params(("id" = String, Path, description = "Provider ID"), LangParam),
    responses((status = 200, body = ProviderDetail), (status = 404, body = ApiError)))]
async fn provider(State(s): St, Path(id): Path<String>, Query(l): Query<LangParam>) -> Response {
    match s.catalog().await.provider(&id, l.lang.as_deref()) {
        Some(d) => Json(d).into_response(),
        None => not_found("provider"),
    }
}

/// Federations (from the eduGAIN database) with this catalog's counts per registrar.
#[utoipa::path(get, path = "/v1/federations", responses((status = 200, body = Vec<FederationView>)))]
async fn federations(State(s): St) -> Json<Vec<FederationView>> {
    Json(s.catalog().await.federations())
}

/// Known entity categories with synonyms and counts, plus every raw tag value seen.
#[utoipa::path(get, path = "/v1/categories", responses((status = 200, body = CategoryReport)))]
async fn categories(State(s): St) -> Json<CategoryReport> {
    Json(s.catalog().await.categories())
}

/// Requested attributes across all services, most requested first.
#[utoipa::path(get, path = "/v1/attributes", responses((status = 200, body = Vec<AttributeView>)))]
async fn attributes(State(s): St) -> Json<Vec<AttributeView>> {
    Json(s.catalog().await.attributes())
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use catalog_core::model::{Service, Snapshot};
    use catalog_core::search::Catalog;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;

    fn app() -> Router {
        let mut s = Service {
            entity_id: "https://sp.example.org/shibboleth".into(),
            id: catalog_core::entity_hash("https://sp.example.org/shibboleth"),
            ..Default::default()
        };
        s.display_names.insert("en".into(), "Example".into());
        let catalog = Catalog::new(Snapshot {
            services: vec![s],
            ..Default::default()
        });
        router(AppState::from_catalog(catalog))
    }

    async fn get_json(path: &str) -> (StatusCode, serde_json::Value) {
        get_json_from(app(), path).await
    }

    async fn get_json_from(app: Router, path: &str) -> (StatusCode, serde_json::Value) {
        let resp = app
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn search_lookup_and_spec() {
        let (st, page) = get_json("/v1/services?q=exam").await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(page["total"], 1);
        let id = page["items"][0]["id"].as_str().unwrap().to_owned();

        let (st, d) = get_json(&format!("/v1/services/%7Bsha1%7D{id}")).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(d["name"], "Example");

        let (st, _) =
            get_json("/v1/lookup?entity_id=https%3A%2F%2Fsp.example.org%2Fshibboleth").await;
        assert_eq!(st, StatusCode::OK);
        let (st, i) = get_json("/v1/interpret?q=example%20services%20that%20request%20email").await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(i["engine"], "rules");
        assert_eq!(i["params"]["q"], "example email");
        let (st, _) = get_json("/v1/interpret?q=%20").await;
        assert_eq!(st, StatusCode::BAD_REQUEST);

        // The one service is not eduGAIN-only, so ask drops that filter and says so.
        let (st, a) = get_json("/v1/ask?q=example%20services%20not%20in%20InCommon").await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(a["total"], 1);
        assert!(a["relaxed"].as_str().unwrap().contains("Availability"));

        let (st, _) = get_json("/v1/services/deadbeef").await;
        assert_eq!(st, StatusCode::NOT_FOUND);

        let (_, spec) = get_json("/openapi.json").await;
        assert!(spec["paths"]["/v1/ask"].is_object() && spec["paths"]["/v1/interpret"].is_object());
        assert!(spec["paths"]["/v1/services"].is_object());
    }

    fn two_federations() -> Router {
        let svc = |id: &str, ra: &str, code: &str| {
            let mut s = Service {
                entity_id: id.into(),
                id: catalog_core::entity_hash(id),
                federation: Some(code.into()),
                ..Default::default()
            };
            s.registration.authority = Some(ra.into());
            s
        };
        let catalog = Catalog::new(Snapshot {
            services: vec![
                svc("https://a/", "https://incommon.org", "INCOMMON"),
                svc("https://b/", "http://www.swamid.se/", "SWAMID"),
                svc("https://c/", "https://www.aai.dfn.de", "DFN-AAI"),
            ],
            ..Default::default()
        });
        router(AppState::from_catalog(catalog))
    }

    #[test]
    fn join_repeated_merges_only_that_key() {
        assert_eq!(join_repeated("", "federation"), "");
        assert_eq!(
            join_repeated("federation=A&q=x", "federation"),
            "federation=A&q=x"
        );
        assert_eq!(
            join_repeated(
                "federation=A&q=x&federation=B%2FC&federation=",
                "federation"
            ),
            "q=x&federation=A,B%2FC,"
        );
        assert_eq!(
            join_repeated("federations=A&federations=B", "federation"),
            "federations=A&federations=B"
        );
    }

    #[tokio::test]
    async fn services_accept_several_federations() {
        let total = |path: &'static str| async move {
            let (st, page) = get_json_from(two_federations(), path).await;
            assert_eq!(st, StatusCode::OK, "{path}");
            page["total"].as_u64().unwrap()
        };
        assert_eq!(total("/v1/services").await, 3);
        assert_eq!(total("/v1/services?federation=SWAMID").await, 1);
        assert_eq!(
            total("/v1/services?federation=https%3A%2F%2Fincommon.org").await,
            1
        );
        assert_eq!(total("/v1/services?federation=INCOMMON,SWAMID").await, 2);
        assert_eq!(total("/v1/services?federation=INCOMMON%2CDFN-AAI").await, 2);
        assert_eq!(
            total("/v1/services?federation=INCOMMON&page=1&federation=SWAMID&federation=DFN-AAI")
                .await,
            3
        );

        let (st, _) = get_json_from(two_federations(), "/v1/services?page=abc").await;
        assert_eq!(st, StatusCode::BAD_REQUEST);

        let (_, spec) = get_json("/openapi.json").await;
        let param = spec["paths"]["/v1/services"]["get"]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "federation")
            .unwrap()
            .clone();
        assert_eq!(param["schema"]["type"], "array");
        assert_eq!(param["schema"]["items"]["type"], "string");
        assert_eq!(param["style"], "form");
        assert_eq!(param["explode"], true);
    }
}
