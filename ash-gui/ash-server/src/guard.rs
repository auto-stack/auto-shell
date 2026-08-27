//! Plan 072 M1 (S-1): loopback + bearer-token + Origin/Host guard.
//!
//! ash-server used to bind `0.0.0.0:3000` with no authentication — anyone on
//! the LAN could POST `/api/run_command` and execute arbitrary commands. The
//! layered fix (decided with the user, 2026-08-26):
//!
//! 1. **Loopback by default** — traffic never leaves the machine, so a
//!    plaintext token cannot be sniffed off the network.
//! 2. **Optional startup token** (`ASH_SERVER_TOKEN`, injected by the shared
//!    dev launcher into both the server and the vite proxy) — required for
//!    non-loopback binds, constant-time compared.
//! 3. **Origin allowlist + loopback Host check** — blocks DNS-rebinding and
//!    cross-site requests from malicious web pages.
//!
//! Not in scope (see plan 070 §6): TLS/remote mode, PID-level peer identity.

use axum::{
    extract::Request,
    http::{
        header::{AUTHORIZATION, HOST, ORIGIN},
        HeaderMap, StatusCode,
    },
    middleware::Next,
    response::{IntoResponse, Response},
};

/// Default browser origins allowed through the guard: the vite dev server
/// (which proxies `/api` → localhost:3000, keeping the browser same-origin).
pub const DEFAULT_ALLOWED_ORIGINS: &[&str] =
    &["http://localhost:5173", "http://127.0.0.1:5173"];

/// Loopback host names accepted in the `Host` header (port ignored).
const LOOPBACK_HOSTS: &[&str] = &["localhost", "127.0.0.1", "[::1]", "::1"];

#[derive(Clone, Debug)]
pub struct GuardConfig {
    /// Shared secret; when set, every request must carry
    /// `Authorization: Bearer <token>`. `None` = tokenless loopback dev mode.
    pub token: Option<String>,
    /// `Origin` allowlist. Requests presenting an Origin outside this list
    /// are refused (403). Requests without an Origin (curl, the vite proxy's
    /// server-to-server hop) are allowed on loopback.
    pub allowed_origins: Vec<String>,
    /// When true the `Host` header must name a loopback address — the
    /// DNS-rebinding signature is a foreign Host on a loopback-bound server.
    /// Turned off automatically for explicit non-loopback binds (where the
    /// mandatory token is the real gate).
    pub require_loopback_host: bool,
}

impl Default for GuardConfig {
    fn default() -> Self {
        GuardConfig {
            token: None,
            allowed_origins: DEFAULT_ALLOWED_ORIGINS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            require_loopback_host: true,
        }
    }
}

impl GuardConfig {
    /// Build from the environment:
    /// - `ASH_SERVER_TOKEN` — shared secret (empty = unset).
    /// - `ASH_SERVER_ORIGIN` — comma-separated allowlist overriding defaults.
    /// - `ASH_SERVER_BIND` — when it names a non-loopback address the Host
    ///   check is relaxed (remote clients reach us by LAN hostname; the
    ///   mandatory token is the gate).
    pub fn from_env() -> Self {
        let mut cfg = GuardConfig::default();
        cfg.token = std::env::var("ASH_SERVER_TOKEN")
            .ok()
            .filter(|s| !s.trim().is_empty());
        if let Ok(list) = std::env::var("ASH_SERVER_ORIGIN") {
            let parsed: Vec<String> = list
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !parsed.is_empty() {
                cfg.allowed_origins = parsed;
            }
        }
        if let Ok(bind) = std::env::var("ASH_SERVER_BIND") {
            if let Ok(addr) = bind.parse::<std::net::SocketAddr>() {
                cfg.require_loopback_host = addr.ip().is_loopback();
            }
        }
        cfg
    }

    /// Validate request headers. `Err` carries the refusal response.
    pub fn check(&self, headers: &HeaderMap) -> Result<(), Response> {
        // ① Bearer token (when configured).
        if let Some(token) = &self.token {
            let expected = format!("Bearer {token}");
            let presented = headers
                .get(AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if !constant_time_eq(presented, expected.as_str()) {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    "ash-server: missing or invalid bearer token",
                )
                    .into_response());
            }
        }

        // ② Origin allowlist (browser requests only).
        if let Some(origin) = headers.get(ORIGIN).and_then(|v| v.to_str().ok()) {
            if !self.allowed_origins.iter().any(|o| o.as_str() == origin) {
                return Err((
                    StatusCode::FORBIDDEN,
                    format!("ash-server: origin '{origin}' not allowed"),
                )
                    .into_response());
            }
        }

        // ③ Loopback Host (DNS-rebinding defense) for loopback-bound servers.
        if self.require_loopback_host {
            let host = headers
                .get(HOST)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if !is_loopback_host(host) {
                return Err((
                    StatusCode::FORBIDDEN,
                    format!("ash-server: host '{host}' not allowed (loopback only)"),
                )
                    .into_response());
            }
        }

        Ok(())
    }
}

/// axum middleware entry — wired in [`crate::http::create_router_with_guard`].
pub async fn guard_middleware(
    axum::extract::State(guard): axum::extract::State<GuardConfig>,
    req: Request,
    next: Next,
) -> Response {
    if let Err(refusal) = guard.check(req.headers()) {
        return refusal;
    }
    next.run(req).await
}

/// Constant-time string equality — no early exit on first mismatching byte.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// `Host: localhost:3000` / `127.0.0.1:3000` / `[::1]:3000` → loopback.
fn is_loopback_host(host: &str) -> bool {
    let host = host.trim();
    // IPv6 form: `[::1]:3000` → strip brackets+port; bare `::1` accepted too.
    let name = if let Some(rest) = host.strip_prefix('[') {
        &rest[..rest.find(']').unwrap_or(0)]
    } else {
        match host.rfind(':') {
            Some(i) if host[i + 1..].chars().all(|c| c.is_ascii_digit()) => &host[..i],
            _ => host,
        }
    };
    LOOPBACK_HOSTS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                axum::http::HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    // ---- ① token ----

    #[test]
    fn token_enforced_when_configured() {
        let g = GuardConfig {
            token: Some("s3cret".into()),
            ..Default::default()
        };
        // Missing header → 401 (checked before the Host rule, so a bare
        // request still reports the auth failure).
        let r = g.check(&headers(&[])).unwrap_err();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        // Wrong token → 401.
        let r = g
            .check(&headers(&[
                (HOST.as_str(), "localhost:3000"),
                (AUTHORIZATION.as_str(), "Bearer wrong"),
            ]))
            .unwrap_err();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        // Correct token + loopback Host → pass.
        assert!(g
            .check(&headers(&[
                (HOST.as_str(), "localhost:3000"),
                (AUTHORIZATION.as_str(), "Bearer s3cret"),
            ]))
            .is_ok());
    }

    #[test]
    fn no_token_loopback_mode_allows() {
        let g = GuardConfig::default();
        assert!(g.check(&headers(&[(HOST.as_str(), "localhost:3000")])).is_ok());
    }

    #[test]
    fn constant_time_eq_basics() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(!constant_time_eq("", "a"));
    }

    // ---- ② Origin ----

    #[test]
    fn foreign_origin_refused() {
        let g = GuardConfig::default();
        // DNS-rebinding / malicious page signature.
        let r = g
            .check(&headers(&[
                (ORIGIN.as_str(), "http://evil.example"),
                (HOST.as_str(), "localhost:3000"),
            ]))
            .unwrap_err();
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
        // Allowlisted origin passes.
        assert!(g
            .check(&headers(&[
                (ORIGIN.as_str(), "http://localhost:5173"),
                (HOST.as_str(), "localhost:3000"),
            ]))
            .is_ok());
        // No Origin (curl / vite server-to-server hop) passes on loopback.
        assert!(g.check(&headers(&[(HOST.as_str(), "127.0.0.1:3000")])).is_ok());
    }

    // ---- ③ Host / DNS rebinding ----

    #[test]
    fn non_loopback_host_refused_in_loopback_mode() {
        let g = GuardConfig::default();
        // Rebind attack: attacker domain resolves to 127.0.0.1.
        let r = g
            .check(&headers(&[(HOST.as_str(), "attacker.example:3000")]))
            .unwrap_err();
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn host_check_relaxed_for_remote_bind() {
        let g = GuardConfig {
            token: Some("t".into()),
            require_loopback_host: false,
            ..Default::default()
        };
        let h = headers(&[
            (HOST.as_str(), "192.168.1.5:3000"),
            (AUTHORIZATION.as_str(), "Bearer t"),
        ]);
        assert!(g.check(&h).is_ok());
    }

    #[test]
    fn loopback_host_forms_accepted() {
        let g = GuardConfig::default();
        for host in ["localhost:3000", "127.0.0.1:3000", "[::1]:3000", "localhost"] {
            let h = headers(&[(HOST.as_str(), host)]);
            assert!(g.check(&h).is_ok(), "{host}");
        }
    }
}
