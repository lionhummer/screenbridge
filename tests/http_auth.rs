use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use screenbridge::{
    config::Config,
    server::{self, App},
};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
fn config() -> Config {
    Config {
        vnc_port: 5901,
        vnc_password: "fixture".into(),
        access_token: "public-fixture-token-not-a-secret-12345".into(),
        ffmpeg: "/usr/bin/ffmpeg".into(),
        listen_port: 8787,
        public_origin: "http://127.0.0.1:8787".into(),
        fps: 30,
        bitrate_kbps: 6000,
    }
}
#[tokio::test]
async fn sign_in_cookie_and_origin_boundary() {
    let c = config();
    let app = server::router(App::new(c.clone(), CancellationToken::new()));
    let r = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(r.headers()["referrer-policy"], "same-origin");
    let b = r.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&b).contains("Access token"));
    for (origin, token, status) in [
        (
            "https://attacker.invalid",
            c.access_token.as_str(),
            StatusCode::FORBIDDEN,
        ),
        (c.public_origin.as_str(), "wrong", StatusCode::UNAUTHORIZED),
    ] {
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/login")
                    .header("origin", origin)
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!("token={token}")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), status);
    }
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header("origin", &c.public_origin)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!("token={}", c.access_token)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::SEE_OTHER);
    let cookie = r.headers()["set-cookie"].to_str().unwrap();
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Strict"));
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .header("cookie", cookie.split(';').next().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let b = r.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&b).contains("/player.js"));
    assert!(!String::from_utf8_lossy(&b).contains(&c.access_token));
    let r = app
        .oneshot(
            Request::builder()
                .uri("/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}
#[tokio::test]
async fn request_size_bound() {
    let app = server::router(App::new(config(), CancellationToken::new()));
    let r = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("x".repeat(2048)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
#[test]
fn validates_origin_and_encoder_limits() {
    let mut c = config();
    assert!(c.validate().is_ok());
    for origin in [
        "http://remote.invalid",
        "https://user@remote.invalid",
        "https://remote.invalid/path",
        "https://remote.invalid/",
    ] {
        c.public_origin = origin.into();
        assert!(c.validate().is_err());
    }
    c = config();
    c.fps = 61;
    assert!(c.validate().is_err());
}
#[cfg(unix)]
#[test]
fn private_configuration_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let f = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(f.path(), "{}").unwrap();
    std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        Config::load(f.path()),
        Err(screenbridge::error::Error::Config(
            "configuration must be owner-only"
        ))
    ));
}
