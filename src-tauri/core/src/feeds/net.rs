//! HTTP transport + SSRF guard for feed pages and article pages.

use crate::error::AppError;
use reqwest::blocking::Client;
use reqwest::redirect;
use std::net::ToSocketAddrs;
use std::sync::LazyLock;

pub(crate) const HTTP_USER_AGENT: &str = "Shiyan/0.1 (+local; educational)";

/// Upper bound for a single HTTP response body (feed XML, article HTML, or
/// LLM JSON). A malicious server must not be able to OOM the app with an
/// unbounded body.
pub const MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;

/// Read a response body with a hard cap. An oversized advertised
/// Content-Length is rejected up front, and the streamed body is counted so a
/// lying server (small header, huge body) cannot exceed the limit either.
pub fn read_limited_bytes(
    mut resp: reqwest::blocking::Response,
) -> Result<Vec<u8>, AppError> {
    if let Some(len) = resp.content_length() {
        if len > MAX_RESPONSE_BYTES as u64 {
            return Err(AppError::msg("响应过大，已跳过"));
        }
    }
    let mut capped = CappedBody {
        out: Vec::new(),
        left: MAX_RESPONSE_BYTES,
        hit_cap: false,
    };
    match resp.copy_to(&mut capped) {
        Ok(_) => Ok(capped.out),
        Err(_) if capped.hit_cap => Err(AppError::msg("响应过大，已跳过")),
        Err(e) => Err(e.into()),
    }
}

/// A `Write` sink that refuses bytes past the cap, so `copy_to` aborts the
/// transfer instead of buffering an unbounded body into memory.
struct CappedBody {
    out: Vec<u8>,
    left: usize,
    hit_cap: bool,
}

impl std::io::Write for CappedBody {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = buf.len().min(self.left);
        self.out.extend_from_slice(&buf[..n]);
        self.left -= n;
        if n < buf.len() {
            self.hit_cap = true;
            return Err(std::io::Error::other("response body exceeds limit"));
        }
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Shared blocking client so connections pool across feeds/pages instead of
/// being rebuilt per request. Every redirect hop is re-checked by the same
/// SSRF guard, so a malicious feed cannot bounce the client to localhost,
/// the LAN, or a cloud metadata endpoint via 30x.
///
/// Built lazily on first use; a build failure surfaces as an `AppError` at
/// the call site instead of panicking (which would poison the `LazyLock`).
pub(crate) static HTTP: LazyLock<Result<Client, String>> = LazyLock::new(|| {
    Client::builder()
        .user_agent(HTTP_USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .redirect(redirect::Policy::custom(|attempt| {
            if ensure_public_http_url(attempt.url().as_str()).is_ok() {
                attempt.follow()
            } else {
                attempt.error("重定向目标不安全，已拦截")
            }
        }))
        .build()
        .map_err(|e| e.to_string())
});

pub(crate) fn http_client() -> Result<Client, AppError> {
    HTTP.as_ref()
        .cloned()
        .map_err(|e| AppError::msg(format!("HTTP 客户端初始化失败：{e}")))
}

/// True for loopback / private / link-local / CGNAT addresses — the ranges an
/// SSRF-style request should never reach.
fn is_blocked_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64.0.0/10 CGNAT
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7 unique-local
                || v6
                    .to_ipv4_mapped()
                    .map(|v4| is_blocked_ip(std::net::IpAddr::V4(v4)))
                    .unwrap_or(false)
        }
    }
}

/// Parse `url` and refuse anything that is not a plain public http(s) target.
/// Feed subscriptions, discovered feeds and imported article pages all pass
/// through here so a malicious source cannot make the app probe `localhost`,
/// the LAN, or the cloud metadata endpoint.
pub fn ensure_public_http_url(url: &str) -> Result<url::Url, AppError> {
    let parsed = url::Url::parse(url).map_err(|_| AppError::msg("链接格式不正确"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(AppError::msg("仅支持 http/https 链接"));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| AppError::msg("链接缺少主机名"))?;
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let lower = bare.to_ascii_lowercase();
    // Integration tests run a fake feed server on loopback. `cfg!(test)` is
    // compile-time false in production builds, so this branch cannot weaken
    // the shipped guard.
    if cfg!(test) && (lower == "localhost" || bare == "127.0.0.1" || bare == "::1") {
        return Ok(parsed);
    }
    if lower == "localhost" || lower.ends_with(".localhost") || lower.ends_with(".local") {
        return Err(AppError::msg("不支持访问本机或内网地址"));
    }
    if let Ok(ip) = bare.parse::<std::net::IpAddr>() {
        if is_blocked_ip(ip) {
            return Err(AppError::msg("不支持访问本机或内网地址"));
        }
    } else if let Ok(addrs) = (bare, 0).to_socket_addrs() {
        // Hostname resolved to a blocked address (DNS-rebinding best effort).
        if addrs.into_iter().any(|a| is_blocked_ip(a.ip())) {
            return Err(AppError::msg("不支持访问本机或内网地址"));
        }
    }
    Ok(parsed)
}

/// Probe a URL: fetch + parse as RSS/Atom. Does not write to DB.
pub fn validate_feed_url(url: &str) -> FeedValidation {
    let url = url.trim();
    let url = match ensure_public_http_url(url) {
        Ok(_) => url,
        Err(e) => {
            return FeedValidation {
                ok: false,
                title: None,
                entry_count: 0,
                error: Some(e.to_string()),
            }
        }
    };
    let client = match http_client() {
        Ok(client) => client,
        Err(e) => {
            return FeedValidation {
                ok: false,
                title: None,
                entry_count: 0,
                error: Some(e.to_string()),
            }
        }
    };
    match client.get(url).send().and_then(|r| r.error_for_status()) {
        Ok(resp) => match read_limited_bytes(resp) {
            Ok(bytes) => match feed_rs::parser::parse(&bytes[..]) {
                Ok(parsed) => FeedValidation {
                    ok: true,
                    title: parsed.title.map(|t| t.content),
                    entry_count: parsed.entries.len(),
                    error: None,
                },
                Err(e) => FeedValidation {
                    ok: false,
                    title: None,
                    entry_count: 0,
                    error: Some(format!("不是有效的 RSS/Atom：{e}")),
                },
            },
            Err(e) => FeedValidation {
                ok: false,
                title: None,
                entry_count: 0,
                error: Some(e.to_string()),
            },
        },
        Err(e) => FeedValidation {
            ok: false,
            title: None,
            entry_count: 0,
            error: Some(e.to_string()),
        },
    }
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct FeedValidation {
    pub ok: bool,
    pub title: Option<String>,
    pub entry_count: usize,
    pub error: Option<String>,
}
