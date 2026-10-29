// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! The one Claude call this project makes, shared by the API (reading searches) and
//! ingest (choosing logos): send a system prompt and a user message, get back JSON
//! that matches a schema.
//!
//! Rust has no official Anthropic SDK, so this speaks the Messages API over HTTP, to
//! one of two providers (`CATALOG_CLAUDE_PROVIDER`):
//!
//! - `anthropic` (default): the Claude API. Credentials: `ANTHROPIC_API_KEY`, or
//!   `ANTHROPIC_API_KEY_SSM_PARAM` naming an SSM SecureString (so the key never sits in
//!   Lambda configuration or Terraform state).
//! - `bedrock`: Claude in Amazon Bedrock, the same Messages API at
//!   `bedrock-mantle.{region}.api.aws`.
//! - `bedrock-runtime`: Claude in Amazon Bedrock through `InvokeModel` on
//!   `bedrock-runtime.{region}.amazonaws.com`, for models a region offers only as
//!   inference profiles (`us.anthropic.claude-haiku-5-5`). Same request body, served
//!   by the same infrastructure; the model goes in the URL instead.
//!
//! Both Bedrock modes are SigV4-signed with the process's AWS credentials (in Lambda,
//! the function's role), so there is no key to manage. Bedrock has no Message Batches
//! API and no server-side refusal fallback.
//!
//! `ANTHROPIC_BASE_URL` overrides the Claude API host, for tests and proxies.

use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use aws_credential_types::provider::{ProvideCredentials, SharedCredentialsProvider};
use serde_json::{json, Value};

const API_VERSION: &str = "2026-10-21";
/// Server-side refusal fallback, `"default"` form: a declined request is re-run on
/// the recommended model for that refusal category instead of failing.
pub const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
pub const DEFAULT_BEDROCK_MODEL: &str = "anthropic.claude-haiku-5-5";
pub const DEFAULT_BEDROCK_RUNTIME_MODEL: &str = "us.anthropic.claude-haiku-5-5";
/// The body field `InvokeModel` takes in place of the `anthropic-version` header.
const BEDROCK_INVOKE_VERSION: &str = "bedrock-2023-05-31";

#[derive(Clone)]
pub struct Claude {
    http: reqwest::Client,
    auth: Auth,
    base: String,
    model: String,
    effort: String,
}

#[derive(Clone)]
enum Auth {
    ApiKey(String),
    Bedrock {
        region: String,
        credentials: SharedCredentialsProvider,
        /// `InvokeModel` on bedrock-runtime rather than the Messages API on mantle.
        invoke: bool,
    },
}

/// One structured-output request. `system` is marked for prompt caching, so keep
/// it identical across requests and put what varies in `user`.
#[derive(Debug, Clone)]
pub struct Request {
    pub system: String,
    pub user: String,
    pub schema: Value,
    pub max_tokens: u32,
}

/// A schema-conforming answer, and the model that actually produced it (a refusal
/// fallback can change it).
#[derive(Debug)]
pub struct Answer {
    pub value: Value,
    pub model: String,
}

impl Claude {
    /// The Claude API with an API key.
    pub fn new(key: String, model: String, effort: String, timeout: Duration) -> Result<Self> {
        Self::build(
            Auth::ApiKey(key),
            "https://api.anthropic.com".into(),
            model,
            effort,
            timeout,
        )
    }

    fn build(
        auth: Auth,
        base: String,
        model: String,
        effort: String,
        timeout: Duration,
    ) -> Result<Self> {
        let base = base_for(&auth, base, std::env::var("ANTHROPIC_BASE_URL").ok());
        Ok(Self {
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(timeout)
                .build()?,
            auth,
            base,
            model,
            effort,
        })
    }

    /// Claude in Amazon Bedrock: `InvokeModel` on bedrock-runtime when `invoke`, else
    /// the Messages API on bedrock-mantle.
    pub fn bedrock(
        region: String,
        credentials: SharedCredentialsProvider,
        invoke: bool,
        model: String,
        effort: String,
        timeout: Duration,
    ) -> Result<Self> {
        let base = if invoke {
            format!("https://bedrock-runtime.{region}.amazonaws.com")
        } else {
            format!("https://bedrock-mantle.{region}.api.aws/anthropic")
        };
        let auth = Auth::Bedrock {
            region,
            credentials,
            invoke,
        };
        Self::build(auth, base, model, effort, timeout)
    }

    /// `None` when no provider is configured (the Claude API without a key).
    /// `model_var`/`effort_var` name the env vars that override the model and effort
    /// for this caller.
    pub async fn from_env(
        model_var: &str,
        effort_var: &str,
        timeout: Duration,
    ) -> Result<Option<Self>> {
        let effort = std::env::var(effort_var).unwrap_or_else(|_| "low".into());
        let model = std::env::var(model_var).ok().filter(|m| !m.is_empty());
        match std::env::var("CATALOG_CLAUDE_PROVIDER").as_deref() {
            Ok(provider @ ("bedrock" | "bedrock-runtime")) => {
                let invoke = provider == "bedrock-runtime";
                let conf = aws_config::load_from_env().await;
                let region = conf
                    .region()
                    .context("Bedrock needs an AWS region (AWS_REGION)")?
                    .to_string();
                let credentials = conf
                    .credentials_provider()
                    .context("Bedrock needs AWS credentials")?;
                let default_model = if invoke {
                    DEFAULT_BEDROCK_RUNTIME_MODEL
                } else {
                    DEFAULT_BEDROCK_MODEL
                };
                let model = model.unwrap_or_else(|| default_model.into());
                Ok(Some(Self::bedrock(
                    region,
                    credentials,
                    invoke,
                    model,
                    effort,
                    timeout,
                )?))
            }
            Ok("anthropic") | Ok("") | Err(_) => {
                let Some(key) = api_key().await? else {
                    return Ok(None);
                };
                let model = model.unwrap_or_else(|| DEFAULT_MODEL.into());
                Ok(Some(Self::new(key, model, effort, timeout)?))
            }
            Ok(other) => bail!(
                "CATALOG_CLAUDE_PROVIDER must be anthropic, bedrock, or bedrock-runtime, not {other}"
            ),
        }
    }

    /// Whether the Message Batches API is available (not on Bedrock).
    pub fn supports_batches(&self) -> bool {
        matches!(self.auth, Auth::ApiKey(_))
    }

    /// Send requests somewhere other than `ANTHROPIC_BASE_URL` (tests).
    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// One structured-output request. `system` is marked for prompt caching, so keep
    /// it identical across calls and put what varies in `user`.
    pub async fn structured(
        &self,
        system: &str,
        user: &str,
        schema: Value,
        max_tokens: u32,
    ) -> Result<Answer> {
        let req = Request {
            system: system.into(),
            user: user.into(),
            schema,
            max_tokens,
        };
        let mut params = self.params(&req, false);
        let url = match &self.auth {
            Auth::Bedrock { invoke: true, .. } => {
                // InvokeModel names the model in the path and the version in the body.
                if let Some(p) = params.as_object_mut() {
                    p.remove("model");
                    p.insert("anthropic_version".into(), BEDROCK_INVOKE_VERSION.into());
                }
                let model = percent_encoding::utf8_percent_encode(
                    &self.model,
                    percent_encoding::NON_ALPHANUMERIC,
                );
                self.url(&format!("/model/{model}/invoke"))
            }
            _ => self.url("/v1/messages"),
        };
        let body = serde_json::to_vec(&params)?;
        let resp = match &self.auth {
            Auth::ApiKey(key) => {
                self.http
                    .post(url)
                    .header("x-api-key", key)
                    .header("anthropic-version", API_VERSION)
                    .header("anthropic-beta", FALLBACK_BETA)
                    .header("content-type", "application/json")
                    .body(body)
                    .send()
                    .await?
            }
            Auth::Bedrock {
                region,
                credentials,
                invoke,
            } => {
                let (service, headers) = if *invoke {
                    (
                        "bedrock",
                        [
                            ("content-type", "application/json"),
                            ("accept", "application/json"),
                        ],
                    )
                } else {
                    (
                        "bedrock-mantle",
                        [
                            ("content-type", "application/json"),
                            ("anthropic-version", API_VERSION),
                        ],
                    )
                };
                let signed =
                    sigv4(credentials, region, service, "POST", &url, &headers, &body).await?;
                let mut rb = self.http.post(url).body(body);
                for (k, v) in headers
                    .into_iter()
                    .chain(signed.iter().map(|(k, v)| (k.as_str(), v.as_str())))
                {
                    rb = rb.header(k, v);
                }
                rb.send().await?
            }
        };
        let status = resp.status();
        let v: Value = resp
            .json()
            .await
            .context("Messages API returned non-JSON")?;
        if !status.is_success() {
            bail!("Messages API {status}: {}", error_message(&v));
        }
        self.answer(&v)
    }

    /// Submit requests to the Message Batches API (half price, results within 24
    /// hours, usually within one). `custom_id`s must match `^[a-zA-Z0-9_-]{1,64}$`.
    pub async fn submit_batch(&self, requests: &[(String, Request)]) -> Result<String> {
        let body = json!({
            "requests": requests
                .iter()
                .map(|(id, r)| json!({ "custom_id": id, "params": self.params(r, true) }))
                .collect::<Vec<_>>(),
        });
        let resp = self
            .http
            .post(self.url("/v1/messages/batches"))
            .header("x-api-key", self.batch_key()?)
            .header("anthropic-version", API_VERSION)
            .timeout(Duration::from_secs(300))
            .json(&body)
            .send()
            .await
            .context("sending the batch (if it reached the API, a batch may exist anyway)")?;
        let (status, v) = json_body(resp.status(), resp.text().await?)?;
        if !status.is_success() {
            bail!("Batches API {status}: {}", error_message(&v));
        }
        v.get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .context("batch has no id")
    }

    /// `in_progress`, `canceling`, or `ended`.
    pub async fn batch_status(&self, id: &str) -> Result<String> {
        let resp = self
            .http
            .get(self.url(&format!("/v1/messages/batches/{id}")))
            .header("x-api-key", self.batch_key()?)
            .header("anthropic-version", API_VERSION)
            .send()
            .await?;
        let (status, v) = json_body(resp.status(), resp.text().await?)?;
        if !status.is_success() {
            bail!("Batches API {status}: {}", error_message(&v));
        }
        v.get("processing_status")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .context("batch has no status")
    }

    /// Results of an ended batch, one per request, in any order.
    pub async fn batch_results(&self, id: &str) -> Result<Vec<(String, Result<Answer>)>> {
        let resp = self
            .http
            .get(self.url(&format!("/v1/messages/batches/{id}/results")))
            .header("x-api-key", self.batch_key()?)
            .header("anthropic-version", API_VERSION)
            .timeout(Duration::from_secs(300))
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            bail!(
                "Batches API {status}: {}",
                body.chars().take(300).collect::<String>()
            );
        }
        let mut out = Vec::new();
        for line in body.lines().filter(|l| !l.trim().is_empty()) {
            let v: Value = serde_json::from_str(line).context("batch result line was not JSON")?;
            let Some(id) = v.get("custom_id").and_then(Value::as_str) else {
                continue;
            };
            let result = match v.pointer("/result/type").and_then(Value::as_str) {
                Some("succeeded") => match v.pointer("/result/message") {
                    Some(m) => self.answer(m),
                    None => Err(anyhow::anyhow!("succeeded without a message")),
                },
                Some("errored") => Err(anyhow::anyhow!(
                    "errored: {}",
                    v.pointer("/result/error/error/message")
                        .or_else(|| v.pointer("/result/error/message"))
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                )),
                Some(other) => Err(anyhow::anyhow!("{other}")),
                None => Err(anyhow::anyhow!("result has no type")),
            };
            out.push((id.to_owned(), result));
        }
        Ok(out)
    }

    fn batch_key(&self) -> Result<&str> {
        match &self.auth {
            Auth::ApiKey(key) => Ok(key),
            Auth::Bedrock { .. } => bail!("Message Batches is not available on Amazon Bedrock"),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base.trim_end_matches('/'))
    }

    /// Messages API parameters. Batches reject server-side `fallbacks`, and run long
    /// enough that the 1-hour prompt cache is the one that pays off.
    fn params(&self, r: &Request, batch: bool) -> Value {
        let cache = if batch {
            json!({ "type": "ephemeral", "ttl": "1h" })
        } else {
            json!({ "type": "ephemeral" })
        };
        let mut p = json!({
            "model": self.model,
            "max_tokens": r.max_tokens,
            "output_config": {
                "effort": self.effort,
                "format": { "type": "json_schema", "schema": r.schema },
            },
            "system": [{ "type": "text", "text": r.system, "cache_control": cache }],
            "messages": [{ "role": "user", "content": r.user }],
        });
        if !batch && matches!(self.auth, Auth::ApiKey(_)) {
            p["fallbacks"] = json!("default");
        }
        p
    }

    /// The schema-conforming JSON from a Messages API response.
    fn answer(&self, v: &Value) -> Result<Answer> {
        match v.get("stop_reason").and_then(Value::as_str) {
            Some("refusal") => bail!("the model declined this request"),
            Some("max_tokens") => bail!("the answer was cut off at max_tokens"),
            _ => {}
        }
        let text: String = v
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect();
        let value = serde_json::from_str(&text).context("model output was not JSON")?;
        let model = v
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(&self.model)
            .to_owned();
        Ok(Answer { value, model })
    }
}

/// Parse a Batches API reply, keeping the status and the start of the body when it
/// is not JSON (a proxy's 413 or 502 page).
fn json_body(status: reqwest::StatusCode, body: String) -> Result<(reqwest::StatusCode, Value)> {
    match serde_json::from_str(&body) {
        Ok(v) => Ok((status, v)),
        Err(_) => bail!(
            "Batches API {status} returned non-JSON: {}",
            body.chars().take(300).collect::<String>()
        ),
    }
}

/// `ANTHROPIC_BASE_URL` is a Claude API setting: an inherited value must not
/// redirect Bedrock traffic.
fn base_for(auth: &Auth, default: String, env: Option<String>) -> String {
    match auth {
        Auth::ApiKey(_) => env.unwrap_or(default),
        Auth::Bedrock { .. } => default,
    }
}

/// SigV4 headers (`authorization`, `x-amz-date`, and the session token if any) for
/// one Bedrock request. `headers` are signed as well and must be sent unchanged.
async fn sigv4(
    credentials: &SharedCredentialsProvider,
    region: &str,
    service: &str,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Result<Vec<(String, String)>> {
    use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningSettings};
    let identity = credentials
        .provide_credentials()
        .await
        .context("loading AWS credentials for Bedrock")?
        .into();
    let params = aws_sigv4::sign::v4::SigningParams::builder()
        .identity(&identity)
        .region(region)
        .name(service)
        .time(SystemTime::now())
        .settings(SigningSettings::default())
        .build()?
        .into();
    let request = SignableRequest::new(
        method,
        url,
        headers.iter().copied(),
        SignableBody::Bytes(body),
    )?;
    let (instructions, _) = sign(request, &params)?.into_parts();
    Ok(instructions
        .headers()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect())
}

fn error_message(v: &Value) -> String {
    // bedrock-runtime reports `{"Message": …}` rather than the Messages API shape.
    v.pointer("/error/message")
        .or_else(|| v.get("message"))
        .or_else(|| v.get("Message"))
        .and_then(Value::as_str)
        .unwrap_or("unknown error")
        .to_owned()
}

async fn api_key() -> Result<Option<String>> {
    if let Some(k) = std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
    {
        return Ok(Some(k));
    }
    let Some(param) = std::env::var("ANTHROPIC_API_KEY_SSM_PARAM")
        .ok()
        .filter(|p| !p.is_empty())
    else {
        return Ok(None);
    };
    let conf = aws_config::load_from_env().await;
    let out = aws_sdk_ssm::Client::new(&conf)
        .get_parameter()
        .name(&param)
        .with_decryption(true)
        .send()
        .await
        .context("reading the Anthropic key from SSM")?;
    Ok(out.parameter().and_then(|p| p.value()).map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use aws_credential_types::Credentials;
    use axum::body::Bytes;
    use axum::http::{HeaderMap, Method, StatusCode, Uri};
    use axum::Router;
    use sha2::{Digest, Sha256};

    use super::*;

    const SECRET: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
    const TOKEN: &str = "session-token-for-tests";

    struct Seen {
        method: Method,
        uri: Uri,
        headers: HeaderMap,
        body: Bytes,
    }

    type Log = Arc<Mutex<Vec<Seen>>>;

    /// A stand-in for every endpoint: records each request and answers `status` with `body`.
    async fn stub(status: u16, body: &'static str) -> (String, Log) {
        let log: Log = Arc::default();
        let seen = log.clone();
        let app = Router::new().fallback(
            move |method: Method, uri: Uri, headers: HeaderMap, body_in: Bytes| {
                let seen = seen.clone();
                async move {
                    seen.lock().unwrap().push(Seen {
                        method,
                        uri,
                        headers,
                        body: body_in,
                    });
                    (
                        StatusCode::from_u16(status).unwrap(),
                        [("content-type", "application/json")],
                        body,
                    )
                }
            },
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}"), log)
    }

    const REPLY: &str =
        r#"{"stop_reason":"end_turn","content":[{"type":"text","text":"{\"reply\":\"hi\"}"}]}"#;

    fn schema() -> Value {
        json!({"type": "object", "properties": {"reply": {"type": "string"}}})
    }

    fn bedrock(base: &str, invoke: bool, model: &str) -> Claude {
        let creds = Credentials::new("AKIDEXAMPLE", SECRET, Some(TOKEN.into()), None, "test");
        Claude::bedrock(
            "us-east-2".into(),
            SharedCredentialsProvider::new(creds),
            invoke,
            model.into(),
            "low".into(),
            Duration::from_secs(5),
        )
        .unwrap()
        .with_base(base)
    }

    fn header<'a>(s: &'a Seen, name: &str) -> Option<&'a str> {
        s.headers.get(name).and_then(|v| v.to_str().ok())
    }

    fn body(s: &Seen) -> Value {
        serde_json::from_slice(&s.body).unwrap()
    }

    fn hmac(key: &[u8], msg: &[u8]) -> Vec<u8> {
        let mut k = if key.len() > 64 {
            Sha256::digest(key).to_vec()
        } else {
            key.to_vec()
        };
        k.resize(64, 0);
        let pad = |b: u8| k.iter().map(|x| x ^ b).collect::<Vec<u8>>();
        let inner = Sha256::digest([pad(0x36), msg.to_vec()].concat());
        Sha256::digest([pad(0x5c), inner.to_vec()].concat()).to_vec()
    }

    /// Recompute the request's SigV4 signature from what actually arrived, without the
    /// signing library, and compare it with the one the client sent.
    fn signature_valid(s: &Seen, secret: &str) -> bool {
        let auth = header(s, "authorization").unwrap();
        let field = |name: &str| {
            auth.split(&format!("{name}="))
                .nth(1)
                .unwrap()
                .split([',', ' '])
                .next()
                .unwrap()
        };
        let scope = field("Credential").split_once('/').unwrap().1;
        let signed = field("SignedHeaders");
        let canonical_headers: String = signed
            .split(';')
            .map(|h| format!("{h}:{}\n", header(s, h).unwrap().trim()))
            .collect();
        // Services other than S3 encode the (already encoded) path a second time.
        let path = s.uri.path().replace('%', "%25");
        let canonical = format!(
            "{}\n{path}\n\n{canonical_headers}\n{signed}\n{}",
            s.method,
            hex::encode(Sha256::digest(&s.body))
        );
        let to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{scope}\n{}",
            header(s, "x-amz-date").unwrap(),
            hex::encode(Sha256::digest(canonical.as_bytes()))
        );
        let mut key = format!("AWS4{secret}").into_bytes();
        for part in scope.split('/') {
            key = hmac(&key, part.as_bytes());
        }
        hex::encode(hmac(&key, to_sign.as_bytes())) == field("Signature")
    }

    #[tokio::test]
    async fn bedrock_runtime_invokes_the_profile_with_a_valid_signature() {
        let (base, log) = stub(200, REPLY).await;
        let claude = bedrock(&base, true, "us.anthropic.claude-haiku-5-5");
        let a = claude
            .structured("sys", "hello", schema(), 64)
            .await
            .unwrap();
        assert_eq!(a.value, json!({"reply": "hi"}));
        assert_eq!(
            a.model, "us.anthropic.claude-haiku-5-5",
            "falls back to the requested model"
        );

        let log = log.lock().unwrap();
        let s = &log[0];
        let path = percent_encoding::percent_decode_str(s.uri.path())
            .decode_utf8()
            .unwrap();
        assert_eq!(path, "/model/us.anthropic.claude-haiku-5-5/invoke");
        let b = body(s);
        assert!(
            b.get("model").is_none(),
            "InvokeModel names the model in the path"
        );
        assert_eq!(b["anthropic_version"], BEDROCK_INVOKE_VERSION);
        assert!(
            b.get("fallbacks").is_none(),
            "Bedrock has no server-side fallback"
        );
        assert_eq!(b["output_config"]["format"]["type"], "json_schema");
        assert_eq!(b["messages"][0]["content"], "hello");

        assert!(header(s, "x-api-key").is_none() && header(s, "anthropic-beta").is_none());
        assert_eq!(header(s, "x-amz-security-token"), Some(TOKEN));
        let auth = header(s, "authorization").unwrap();
        assert!(auth.contains("/us-east-2/bedrock/aws4_request"), "{auth}");
        assert!(
            signature_valid(s, SECRET),
            "signature does not match the request"
        );
        assert!(
            !signature_valid(s, "not-the-secret"),
            "the check must notice a wrong key"
        );
    }

    #[tokio::test]
    async fn bedrock_messages_api_signs_for_mantle() {
        let (base, log) = stub(200, REPLY).await;
        let claude = bedrock(&base, false, "anthropic.claude-haiku-4-5");
        claude
            .structured("sys", "hello", schema(), 64)
            .await
            .unwrap();

        let log = log.lock().unwrap();
        let s = &log[0];
        assert_eq!(s.uri.path(), "/v1/messages");
        let b = body(s);
        assert_eq!(b["model"], "anthropic.claude-haiku-4-5");
        assert!(b.get("fallbacks").is_none() && b.get("anthropic_version").is_none());
        assert_eq!(header(s, "anthropic-version"), Some(API_VERSION));
        let auth = header(s, "authorization").unwrap();
        assert!(
            auth.contains("/us-east-2/bedrock-mantle/aws4_request"),
            "{auth}"
        );
        assert!(
            auth.contains("anthropic-version"),
            "the version header is signed: {auth}"
        );
        assert!(signature_valid(s, SECRET));
    }

    #[tokio::test]
    async fn api_key_requests_keep_the_key_and_fallback() {
        let (base, log) = stub(200, REPLY).await;
        let claude = Claude::new(
            "test-key".into(),
            "claude-opus-5-5".into(),
            "low".into(),
            Duration::from_secs(5),
        )
        .unwrap()
        .with_base(base);
        claude
            .structured("sys", "hello", schema(), 64)
            .await
            .unwrap();

        let log = log.lock().unwrap();
        let s = &log[0];
        assert_eq!(s.uri.path(), "/v1/messages");
        assert_eq!(header(s, "x-api-key"), Some("test-key"));
        assert_eq!(header(s, "anthropic-beta"), Some(FALLBACK_BETA));
        assert!(header(s, "authorization").is_none());
        assert_eq!(body(s)["fallbacks"], "default");
    }

    #[tokio::test]
    async fn bedrock_errors_carry_the_service_message() {
        let (base, _) = stub(
            403,
            r#"{"Message":"User is not authorized to perform: bedrock:InvokeModel"}"#,
        )
        .await;
        let err = bedrock(&base, true, "us.anthropic.claude-haiku-5-5")
            .structured("sys", "hello", schema(), 64)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("403") && err.contains("not authorized"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn bedrock_has_no_batches_and_sends_nothing_for_them() {
        let (base, log) = stub(200, "{}").await;
        let claude = bedrock(&base, true, "us.anthropic.claude-haiku-5-5");
        assert!(!claude.supports_batches());
        let req = Request {
            system: "s".into(),
            user: "u".into(),
            schema: schema(),
            max_tokens: 64,
        };
        let err = claude
            .submit_batch(&[("a".into(), req)])
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("not available on Amazon Bedrock"), "{err}");
        assert!(claude.batch_status("msgbatch_x").await.is_err());
        assert!(log.lock().unwrap().is_empty());
    }

    #[test]
    fn inherited_base_url_only_applies_to_the_claude_api() {
        let creds = SharedCredentialsProvider::new(Credentials::new("a", "b", None, None, "t"));
        let bedrock = Auth::Bedrock {
            region: "us-east-2".into(),
            credentials: creds,
            invoke: true,
        };
        let env = Some("https://api.anthropic.com".to_owned());
        let runtime = "https://bedrock-runtime.us-east-2.amazonaws.com".to_owned();
        assert_eq!(base_for(&bedrock, runtime.clone(), env.clone()), runtime);
        let key = Auth::ApiKey("k".into());
        assert_eq!(
            base_for(
                &key,
                "https://api.anthropic.com".into(),
                Some("http://proxy".into())
            ),
            "http://proxy"
        );
        assert_eq!(
            base_for(&key, "https://api.anthropic.com".into(), None),
            "https://api.anthropic.com"
        );
    }

    #[test]
    fn error_messages_from_each_api_shape() {
        assert_eq!(error_message(&json!({"error": {"message": "a"}})), "a");
        assert_eq!(error_message(&json!({"message": "b"})), "b");
        assert_eq!(error_message(&json!({"Message": "c"})), "c");
        assert_eq!(error_message(&json!({})), "unknown error");
    }
}
