mod common;
use futures_util::{SinkExt, StreamExt};
use screenbridge::{config::Config, rfb::PixelLayout, server};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use tokio_util::sync::CancellationToken;
async fn cookie(port: u16, origin: &str) -> String {
    let mut tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let body = "token=public-fixture-token-not-a-secret-12345";
    let req = format!(
        "POST /login HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    tcp.write_all(req.as_bytes()).await.unwrap();
    let mut b = Vec::new();
    tcp.read_to_end(&mut b).await.unwrap();
    let text = String::from_utf8(b).unwrap();
    assert!(text.starts_with("HTTP/1.1 303"));
    text.lines()
        .find_map(|l| l.strip_prefix("set-cookie: "))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}
#[tokio::test]
#[ignore = "requires hardware FFmpeg on macOS"]
async fn exclusive_control_and_acknowledged_release() {
    let ffmpeg = std::env::var("SCREENBRIDGE_FFMPEG").unwrap();
    let (vnc_port, mut inputs, fixture) = common::fixture(PixelLayout::Bgra, false, false).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let origin = format!("http://127.0.0.1:{port}");
    let c = Config {
        vnc_port,
        vnc_password: "fixture".into(),
        access_token: "public-fixture-token-not-a-secret-12345".into(),
        ffmpeg: ffmpeg.into(),
        listen_port: port,
        public_origin: origin.clone(),
        fps: 30,
        bitrate_kbps: 6000,
    };
    let cancel = CancellationToken::new();
    let app = server::App::new(c, cancel.clone());
    let stop = cancel.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, server::router(app))
            .with_graceful_shutdown(async move { stop.cancelled().await })
            .await
            .unwrap()
    });
    let cookie = cookie(port, &origin).await;
    let request = || {
        let mut r = format!("ws://127.0.0.1:{port}/stream")
            .into_client_request()
            .unwrap();
        r.headers_mut().insert("origin", origin.parse().unwrap());
        r.headers_mut().insert("cookie", cookie.parse().unwrap());
        r
    };
    let (mut controller, _) = connect_async(request()).await.unwrap();
    let role = tokio::time::timeout(Duration::from_secs(10), controller.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(role.to_text().unwrap().contains("controller"));
    let (mut viewer, _) = connect_async(request()).await.unwrap();
    let role = tokio::time::timeout(Duration::from_secs(10), viewer.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(role.to_text().unwrap().contains("viewer"));
    viewer
        .send(Message::Text(r#"{"t":"key","k":65293,"d":true}"#.into()))
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), inputs.recv())
            .await
            .is_err()
    );
    controller
        .send(Message::Text(r#"{"t":"key","k":65293,"d":true}"#.into()))
        .await
        .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), inputs.recv())
            .await
            .unwrap()
            .unwrap()[0],
        1
    );
    controller.close(None).await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), inputs.recv())
            .await
            .unwrap()
            .unwrap()[0],
        0
    );
    viewer.close(None).await.unwrap();
    drop(controller);
    drop(viewer);
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    fixture.abort();
}
