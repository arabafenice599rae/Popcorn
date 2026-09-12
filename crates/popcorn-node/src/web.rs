//! The explorer and wallet the node serves at `/` (SPEC.md §3.2, §13.3).
//!
//! Serving a page is not consensus, and nothing here can influence a state root. It is here
//! for one reason: the client flow of §3.2 puts a signature and a timelock encryption in the
//! user's browser, and a page fetched from a third party is a page that can be swapped for
//! one that signs something else. Serving it from the node makes the page same-origin with
//! the API it talks to, so an operator hands out exactly one address and there is no CORS
//! surface to widen.
//!
//! The bundle is compiled in rather than read from disk: a node is a single binary, and an
//! operator should not have to keep a directory of assets next to it — or be able to lose
//! one. `web/dist` is built by `web/build.sh` and committed; `web/test/browser-path.sh`
//! checks that the committed bundle still matches its sources.

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

const INDEX_HTML: &str = include_str!("../../../web/dist/index.html");
const APP_JS: &str = include_str!("../../../web/dist/app.js");
const APP_CSS: &str = include_str!("../../../web/dist/app.css");
const LOGO: &[u8] = include_bytes!("../../../web/dist/logo.jpg");

/// The asset routes. Mounted under the API router, so `/tx` and friends still win.
pub fn routes() -> Router {
    Router::new()
        .route("/", get(|| async { html(INDEX_HTML) }))
        .route("/index.html", get(|| async { html(INDEX_HTML) }))
        .route(
            "/app.js",
            get(|| async { asset("text/javascript; charset=utf-8", APP_JS.as_bytes()) }),
        )
        .route(
            "/app.css",
            get(|| async { asset("text/css; charset=utf-8", APP_CSS.as_bytes()) }),
        )
        .route("/logo.jpg", get(|| async { asset("image/jpeg", LOGO) }))
}

fn html(body: &'static str) -> Response {
    // No caching for the page itself: the assets carry the version, and an operator who
    // upgrades should not have to explain a stale tab.
    (
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
            // The page loads nothing from anywhere else, and says so: a wallet page that can
            // be made to fetch a script from elsewhere is a wallet page that can be made to
            // sign something else.
            (
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static(
                    "default-src 'self'; script-src 'self'; style-src 'self'; \
                     img-src 'self' data:; connect-src 'self' ws: wss:; \
                     base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
                ),
            ),
        ],
        body,
    )
        .into_response()
}

fn asset(content_type: &'static str, body: &'static [u8]) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=300"),
            ),
        ],
        body,
    )
        .into_response()
}
