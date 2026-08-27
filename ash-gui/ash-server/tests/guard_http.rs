//! Plan 072 M1: guard 路由级测试 — 打真实 `create_router_with_guard`
//! (含 Shell worker),覆盖验收清单:无 token 401、伪 Origin 403、
//! open_path 元字符载荷 400。

use axum::{
    body::Body,
    http::{header::{AUTHORIZATION, HOST, ORIGIN}, Request, StatusCode},
};
use tower::ServiceExt;

use ash_server::guard::GuardConfig;
use ash_server::http::create_router_with_guard;

fn req(method: &str, uri: &str, headers: &[(&str, &str)], body: Option<String>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        builder = builder.header(*k, *v);
    }
    let body = match body {
        Some(json) => Body::from(json),
        None => Body::empty(),
    };
    builder.body(body).unwrap()
}

#[tokio::test]
async fn tokenless_loopback_get_passes() {
    let router = create_router_with_guard(ash_server::spawn(), GuardConfig::default());
    let resp = router
        .oneshot(req("GET", "/api/history", &[(HOST.as_str(), "localhost:3000")], None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn missing_or_wrong_token_is_401() {
    let guard = GuardConfig {
        token: Some("s3cret".into()),
        ..Default::default()
    };
    let router = create_router_with_guard(ash_server::spawn(), guard);
    for headers in [
        vec![(HOST.as_str(), "localhost:3000")],
        vec![
            (HOST.as_str(), "localhost:3000"),
            (AUTHORIZATION.as_str(), "Bearer wrong"),
        ],
    ] {
        let resp = router
            .clone()
            .oneshot(req("GET", "/api/history", &headers, None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn foreign_origin_is_403() {
    let router = create_router_with_guard(ash_server::spawn(), GuardConfig::default());
    let resp = router
        .oneshot(req(
            "GET",
            "/api/history",
            &[
                (HOST.as_str(), "localhost:3000"),
                (ORIGIN.as_str(), "http://evil.example"),
            ],
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn rebind_host_is_403() {
    // DNS rebinding: attacker domain resolves to 127.0.0.1.
    let router = create_router_with_guard(ash_server::spawn(), GuardConfig::default());
    let resp = router
        .oneshot(req("GET", "/api/history", &[(HOST.as_str(), "attacker.example:3000")], None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn open_path_metachar_payload_is_400() {
    // S-2: old handler ran `cmd /C start "" x & calc`.
    let router = create_router_with_guard(ash_server::spawn(), GuardConfig::default());
    let resp = router
        .oneshot(req(
            "POST",
            "/api/open_path",
            &[
                (HOST.as_str(), "localhost:3000"),
                ("content-type", "application/json"),
            ],
            Some(r#"{"path":"x & calc"}"#.into()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
