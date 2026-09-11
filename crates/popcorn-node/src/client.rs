//! A minimal HTTP/1.1 client for the CLI.
//!
//! Hand-rolled for the same reason as the argument parser and the base64 codec: §2 fixes the
//! dependency list, and a convenience command is not a reason to widen an audit surface. It
//! speaks plain HTTP to a node — anything facing the public internet belongs behind a reverse
//! proxy, which is where TLS belongs too.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub struct Url {
    pub host: String,
    pub port: u16,
    pub path: String,
}

impl Url {
    /// Parse `http://host[:port]/path`. HTTPS is deliberately unsupported here.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let rest = raw
            .strip_prefix("http://")
            .ok_or_else(|| format!("{raw}: only http:// URLs are supported by this client"))?;
        let (authority, path) = match rest.find('/') {
            Some(index) => (&rest[..index], &rest[index..]),
            None => (rest, "/"),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (
                host.to_string(),
                port.parse().map_err(|_| format!("bad port in {raw}"))?,
            ),
            None => (authority.to_string(), 80u16),
        };
        Ok(Self {
            host,
            port,
            path: path.to_string(),
        })
    }

    pub fn join(&self, suffix: &str) -> String {
        format!("http://{}:{}{}", self.host, self.port, suffix)
    }
}

fn request(url: &Url, method: &str, body: Option<&str>) -> Result<String, String> {
    let mut stream = TcpStream::connect((url.host.as_str(), url.port))
        .map_err(|e| format!("connect {}:{}: {e}", url.host, url.port))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| e.to_string())?;

    let mut head = format!(
        "{method} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
        url.path, url.host
    );
    if let Some(body) = body {
        head.push_str("Content-Type: application/json\r\n");
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");

    stream
        .write_all(head.as_bytes())
        .map_err(|e| e.to_string())?;
    if let Some(body) = body {
        stream
            .write_all(body.as_bytes())
            .map_err(|e| e.to_string())?;
    }

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&response).to_string();

    let (headers, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "malformed HTTP response".to_string())?;

    // Connection: close means the server may or may not chunk; handle the common case.
    let body = if headers
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        dechunk(body)
    } else {
        body.to_string()
    };

    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("000");
    if !status.starts_with('2') {
        return Err(format!("HTTP {status}: {body}"));
    }
    Ok(body)
}

fn dechunk(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    loop {
        let Some((size_line, remainder)) = rest.split_once("\r\n") else {
            break;
        };
        let Ok(size) = usize::from_str_radix(size_line.trim(), 16) else {
            break;
        };
        if size == 0 || remainder.len() < size {
            break;
        }
        out.push_str(&remainder[..size]);
        rest = remainder[size..].strip_prefix("\r\n").unwrap_or("");
    }
    out
}

pub fn get(url: &str) -> Result<serde_json::Value, String> {
    let parsed = Url::parse(url)?;
    let body = request(&parsed, "GET", None)?;
    serde_json::from_str(&body).map_err(|e| format!("{e}: {body}"))
}

pub fn post(url: &str, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    let parsed = Url::parse(url)?;
    let text = body.to_string();
    let response = request(&parsed, "POST", Some(&text))?;
    serde_json::from_str(&response).map_err(|e| format!("{e}: {response}"))
}
