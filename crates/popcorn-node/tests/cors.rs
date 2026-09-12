//! Cross-origin headers, over a real socket (SPEC.md §13.3, `src/cors.rs`).
//!
//! The policy decision is unit-tested next to the code; what this checks is the part that
//! only shows up on the wire — that a refused origin gets no header at all rather than a
//! header saying "no", that a preflight is answered without reaching a handler, and that a
//! node started without the flag stays silent. Those are exactly the properties that regress
//! quietly, because a browser's error message never reaches the operator's logs.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;

use ed25519_dalek::SigningKey;
use popcorn_core::genesis::GenesisConfig;
use popcorn_node::api::{router, NodeApi};
use popcorn_node::chain::Chain;
use popcorn_node::cors::CorsPolicy;
use popcorn_node::mempool::Mempool;
use popcorn_timelock::static_provider::StaticTimelock;
use popcorn_timelock::TimelockProvider;
use tokio::sync::{broadcast, Mutex};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "popcorn-cors-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Start a node API on an ephemeral port and return its address.
fn serve(dir: &TempDir, cors: Option<CorsPolicy>) -> String {
    let node_key = SigningKey::from_bytes(&[7u8; 32]);
    let foundation_key = SigningKey::from_bytes(&[9u8; 32]);
    let config = GenesisConfig {
        genesis_drand_round: 1_000,
        node_pubkey: node_key.verifying_key().to_bytes(),
        foundation_pubkey: foundation_key.verifying_key().to_bytes(),
    };
    let chain = Chain::initialize(&dir.0.join("popcorn.redb"), config).unwrap();

    let (address_tx, address_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let timelock: Arc<dyn TimelockProvider> =
                Arc::new(StaticTimelock::new([0u8; 32], Vec::new(), 0, 3));
            let (blocks, _) = broadcast::channel(8);
            let api = Arc::new(NodeApi {
                chain: Arc::new(Mutex::new(chain)),
                mempool: Arc::new(Mempool::new()),
                timelock,
                node_key,
                blocks,
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            address_tx
                .send(listener.local_addr().unwrap().to_string())
                .unwrap();
            axum::serve(listener, router(api, false, cors))
                .await
                .unwrap();
        });
    });
    address_rx.recv().unwrap()
}

/// One raw HTTP/1.1 request, returning the whole response as text.
fn request(address: &str, request_line: &str, headers: &[&str]) -> String {
    let mut stream = TcpStream::connect(address).unwrap();
    let mut message = format!("{request_line}\r\nHost: {address}\r\nConnection: close\r\n");
    for header in headers {
        message.push_str(header);
        message.push_str("\r\n");
    }
    message.push_str("\r\n");
    stream.write_all(message.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response.to_lowercase()
}

#[test]
fn an_allowed_origin_is_echoed_and_a_refused_one_gets_nothing() {
    let dir = TempDir::new("list");
    let policy = CorsPolicy::parse("https://frontend.example").unwrap();
    let address = serve(&dir, Some(policy));

    let allowed = request(
        &address,
        "GET /head HTTP/1.1",
        &["Origin: https://frontend.example"],
    );
    assert!(allowed.contains("access-control-allow-origin: https://frontend.example"));
    // A list policy makes the answer origin-dependent, so caches must be told.
    assert!(allowed.contains("vary: origin"));

    // Not "allow-origin: null" — no header at all. A header saying no is still a header a
    // proxy can rewrite; absence is the unambiguous answer.
    let refused = request(
        &address,
        "GET /head HTTP/1.1",
        &["Origin: https://evil.example"],
    );
    assert!(!refused.contains("access-control-allow-origin"));

    // A suffix of an allowed origin is a different origin. `frontend.example.evil` is the
    // classic way a prefix match gets exploited.
    let lookalike = request(
        &address,
        "GET /head HTTP/1.1",
        &["Origin: https://frontend.example.evil"],
    );
    assert!(!lookalike.contains("access-control-allow-origin"));
}

#[test]
fn a_preflight_is_answered_without_reaching_a_handler() {
    let dir = TempDir::new("preflight");
    let address = serve(&dir, Some(CorsPolicy::parse("*").unwrap()));

    // `POST /tx` has no OPTIONS route, so without the middleware this would be a 405 and
    // every third-party submission would fail at the preflight.
    let response = request(
        &address,
        "OPTIONS /tx HTTP/1.1",
        &[
            "Origin: https://anything.example",
            "Access-Control-Request-Method: POST",
            "Access-Control-Request-Headers: content-type",
        ],
    );
    assert!(response.starts_with("http/1.1 204"));
    assert!(response.contains("access-control-allow-origin: *"));
    assert!(response.contains("access-control-allow-methods: get, post, options"));
    assert!(response.contains("access-control-allow-headers: content-type, accept"));
}

#[test]
fn credentials_are_never_allowed() {
    let dir = TempDir::new("credentials");
    let address = serve(&dir, Some(CorsPolicy::parse("*").unwrap()));
    let response = request(
        &address,
        "GET /head HTTP/1.1",
        &["Origin: https://a.example"],
    );
    // There is no authorization on this API, so there are no credentials to send. Claiming
    // otherwise would also make `*` illegal per the CORS specification.
    assert!(!response.contains("access-control-allow-credentials"));
}

#[test]
fn without_the_flag_no_cross_origin_header_is_sent() {
    let dir = TempDir::new("off");
    let address = serve(&dir, None);
    let response = request(
        &address,
        "GET /head HTTP/1.1",
        &["Origin: https://a.example"],
    );
    assert!(response.starts_with("http/1.1 200"));
    assert!(!response.contains("access-control"));
}
