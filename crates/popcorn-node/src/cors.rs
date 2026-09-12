//! Cross-origin access for third-party front ends (SPEC.md §13.3).
//!
//! Off by default, and that default is not caution for its own sake: the page this node
//! serves is same-origin with its API, so it needs nothing here. The flag exists for the
//! other case — somebody building their own explorer or wallet, hosted somewhere else, whose
//! browser will otherwise refuse to read this API at all.
//!
//! What CORS is *not*, here: a security boundary. This API has no cookies, no sessions and no
//! authorization — every endpoint answers the same to everyone, and `POST /tx` accepts bytes
//! from anyone by design (§5.1: collection is blind). A page that cannot reach the API from
//! JavaScript can still reach it from its own backend, so refusing the header protects
//! nothing; it only decides whether third-party pages have to run a proxy. That is why
//! credentials are never allowed: there are none to send, and `Allow-Credentials` would be a
//! claim that there are.
//!
//! `/stream` is unaffected either way — WebSocket handshakes are not subject to CORS.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Which origins may read this API from a browser.
#[derive(Debug, Clone)]
pub enum CorsPolicy {
    /// `--cors '*'`: any origin. The honest setting for a public node.
    Any,
    /// `--cors https://a.example,https://b.example`: exactly these, echoed back.
    Origins(Vec<String>),
}

impl CorsPolicy {
    /// Parse the flag value. An empty list is rejected rather than silently allowing nothing.
    pub fn parse(value: &str) -> Result<Self, String> {
        if value.trim() == "*" {
            return Ok(CorsPolicy::Any);
        }
        let origins: Vec<String> = value
            .split(',')
            .map(|origin| origin.trim().trim_end_matches('/').to_string())
            .filter(|origin| !origin.is_empty())
            .collect();
        if origins.is_empty() {
            return Err("--cors takes '*' or a comma-separated list of origins".to_string());
        }
        // An origin is scheme://host[:port] and nothing else — a path here would never match
        // what a browser sends, so it is a typo worth refusing rather than debugging later.
        for origin in &origins {
            if !(origin.starts_with("http://") || origin.starts_with("https://")) {
                return Err(format!(
                    "`{origin}` is not an origin: expected http:// or https://"
                ));
            }
            if origin[8..].contains('/') {
                return Err(format!("`{origin}` is not an origin: it has a path"));
            }
        }
        Ok(CorsPolicy::Origins(origins))
    }

    /// The value to send back for this request's `Origin`, if it is allowed one.
    fn allow(&self, origin: &str) -> Option<HeaderValue> {
        match self {
            CorsPolicy::Any => HeaderValue::from_static("*").into(),
            CorsPolicy::Origins(origins) => origins
                .iter()
                .any(|allowed| allowed == origin)
                .then(|| HeaderValue::from_str(origin).ok())
                .flatten(),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            CorsPolicy::Any => "any origin".to_string(),
            CorsPolicy::Origins(origins) => origins.join(", "),
        }
    }
}

const ALLOW_METHODS: HeaderValue = HeaderValue::from_static("GET, POST, OPTIONS");
const ALLOW_HEADERS: HeaderValue = HeaderValue::from_static("content-type, accept");
const MAX_AGE: HeaderValue = HeaderValue::from_static("86400");

/// The middleware. Applied to the whole router, so it also answers preflights for paths that
/// would otherwise be a 405.
pub async fn layer(
    State(policy): State<Arc<CorsPolicy>>,
    request: Request,
    next: Next,
) -> Response {
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    let allow = origin.as_deref().and_then(|origin| policy.allow(origin));
    let preflight = request.method() == Method::OPTIONS;

    let mut response = if preflight {
        // A preflight is answered here and never reaches a handler: nothing is read and
        // nothing is submitted by an OPTIONS request.
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(request).await
    };

    if let Some(allow) = allow {
        let headers = response.headers_mut();
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, allow);
        headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, ALLOW_METHODS);
        headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, ALLOW_HEADERS);
        headers.insert(header::ACCESS_CONTROL_MAX_AGE, MAX_AGE);
        // Caches must key on the origin: with a list policy the answer differs per origin.
        if matches!(*policy, CorsPolicy::Origins(_)) {
            // `HeaderValue::from_name` is `http`'s own conversion — a header name is always
            // a valid header value.
            headers.insert(header::VARY, HeaderValue::from_name(header::ORIGIN));
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_allows_anything() {
        let policy = CorsPolicy::parse("*").expect("'*' parses");
        assert_eq!(policy.allow("https://anything.example").unwrap(), "*");
    }

    #[test]
    fn a_list_allows_only_its_members() {
        let policy = CorsPolicy::parse("https://a.example, https://b.example/").expect("parses");
        assert_eq!(
            policy.allow("https://a.example").unwrap(),
            "https://a.example"
        );
        // Not a member, and not a prefix match either — `https://a.example.evil` must fail.
        assert!(policy.allow("https://a.example.evil").is_none());
        assert!(policy.allow("https://c.example").is_none());
    }

    #[test]
    fn malformed_origins_are_refused_at_startup() {
        assert!(CorsPolicy::parse("").is_err());
        assert!(CorsPolicy::parse("a.example").is_err());
        assert!(CorsPolicy::parse("https://a.example/path").is_err());
    }
}
