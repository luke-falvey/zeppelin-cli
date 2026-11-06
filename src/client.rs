//! Thin HTTP client for the Zeppelin REST API.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::Method;
use reqwest::blocking::{Client as Http, RequestBuilder};
use serde_json::Value;

use crate::config::{Resolved, api_root};

pub struct Client {
    http: Http,
    api: reqwest::Url,
}

/// An error response from the server, with the server's own status/message kept.
#[derive(Debug)]
pub struct ApiError {
    pub http_status: u16,
    pub body: Value,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}", self.http_status)?;
        if let Some(s) = self.body.get("status").and_then(Value::as_str) {
            write!(f, " {s}")?;
        }
        match self.body.get("message").and_then(Value::as_str) {
            Some(m) if !m.is_empty() => write!(f, ": {m}")?,
            _ => {}
        }
        if let Some(b) = self.body.get("body").filter(|b| !b.is_null()) {
            write!(
                f,
                "\n{}",
                serde_json::to_string_pretty(b).unwrap_or_default()
            )?;
        } else if let Some(t) = self.body.as_str().map(str::trim).filter(|t| !t.is_empty()) {
            if t.starts_with('<') {
                // Usually Jetty's HTML error page: the endpoint doesn't exist on this server.
                write!(
                    f,
                    ": non-JSON (HTML) response; check base_url, or this server may not support the endpoint"
                )?;
            } else {
                let short: String = t.chars().take(500).collect();
                write!(f, ": {short}")?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for ApiError {}

impl Client {
    /// Build the client and, when a username is configured, log in via
    /// `POST /api/login` so the session cookie is used for later requests.
    pub fn connect(conf: &Resolved, timeout: Duration) -> Result<Self> {
        let http = Http::builder()
            .cookie_store(true)
            .timeout(timeout)
            .user_agent(concat!("zeppelin-cli/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("could not build HTTP client")?;
        let api = reqwest::Url::parse(&api_root(&conf.base_url))
            .with_context(|| format!("invalid base_url '{}'", conf.base_url))?;
        let client = Client { http, api };

        if let Some(user) = &conf.username {
            let password = conf.password.as_deref().unwrap_or("");
            let resp = client
                .http
                .post(client.url(&["login"]))
                .form(&[("userName", user.as_str()), ("password", password)])
                .send()
                .with_context(|| format!("could not reach {}", client.api))?;
            let status = resp.status();
            if !status.is_success() {
                bail!("login as '{user}' failed (HTTP {})", status.as_u16());
            }
        }
        Ok(client)
    }

    fn url(&self, segments: &[&str]) -> reqwest::Url {
        let mut url = self.api.clone();
        url.path_segments_mut()
            .expect("http(s) URL")
            .pop_if_empty()
            .extend(segments);
        url
    }

    fn request(&self, method: Method, segments: &[&str]) -> RequestBuilder {
        self.http.request(method, self.url(segments))
    }

    /// Send a request and return the parsed JSON envelope
    /// (`{"status": ..., "message": ..., "body": ...}`).
    pub fn call(
        &self,
        method: Method,
        segments: &[&str],
        query: &[(&str, String)],
        body: Option<RequestBody>,
    ) -> Result<Value> {
        let mut req = self.request(method.clone(), segments);
        if !query.is_empty() {
            req = req.query(query);
        }
        req = match body {
            Some(RequestBody::Json(v)) => req.json(&v),
            Some(RequestBody::Raw(s)) => req.header("Content-Type", "application/json").body(s),
            None => req,
        };
        let url = self.url(segments);
        let resp = req
            .send()
            .with_context(|| format!("{method} {url} failed"))?;
        let status = resp.status();
        let text = resp.text().context("could not read response body")?;
        let parsed: Value = if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        };
        if !status.is_success() {
            return Err(ApiError {
                http_status: status.as_u16(),
                body: parsed,
            }
            .into());
        }
        Ok(parsed)
    }

    pub fn get(&self, segments: &[&str]) -> Result<Value> {
        self.call(Method::GET, segments, &[], None)
    }
}

pub enum RequestBody {
    Json(Value),
    /// Pre-serialised JSON, sent as-is (used for note import so the
    /// user's file reaches the server byte-for-byte).
    Raw(String),
}
