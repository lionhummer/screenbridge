mod common;
use common::fixture;
use screenbridge::rfb::{self, Input, PixelLayout};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
#[tokio::test]
async fn authenticates_fragmented_capture_and_input() {
    for layout in [PixelLayout::Bgra, PixelLayout::Rgba] {
        let (port, mut inputs, server) = fixture(layout, false, false).await;
        let cancel = CancellationToken::new();
        let mut c = rfb::connect(port, "fixture", cancel.clone()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), c.frames.changed())
            .await
            .unwrap()
            .unwrap();
        let frame = c.frames.borrow().clone().unwrap();
        assert_eq!(frame.layout, layout);
        assert_eq!(frame.pixels.len(), 12288);
        assert_eq!(frame.pixels[1], 80);
        c.input
            .send(Input::Key {
                keysym: 0xff0d,
                down: true,
            })
            .await
            .unwrap();
        let b = tokio::time::timeout(Duration::from_secs(2), inputs.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(b, [1, 0, 0, 0, 0, 255, 13]);
        cancel.cancel();
        assert!((&mut c.task).await.as_mut().is_ok());
        server.abort();
    }
}
#[tokio::test]
async fn rejects_pixel_format() {
    let (port, _, task) = fixture(PixelLayout::Bgra, false, true).await;
    assert!(
        rfb::connect(port, "fixture", CancellationToken::new())
            .await
            .is_err()
    );
    task.abort();
}
#[tokio::test]
async fn out_of_bounds_rect_closes_capture() {
    let (port, _, server) = fixture(PixelLayout::Bgra, true, false).await;
    let mut c = rfb::connect(port, "fixture", CancellationToken::new())
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), &mut c.task)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    assert!(c.frames.borrow().is_none());
    server.abort();
}
#[tokio::test]
async fn cancelled_handshake_drops_tcp() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        rfb::connect(port, "fixture", cancel).await,
        Err(screenbridge::error::Error::Cancelled)
    ));
}
#[tokio::test]
async fn dropping_capture_closes_connection() {
    let (port, _, server) = fixture(PixelLayout::Bgra, false, false).await;
    let c = rfb::connect(port, "fixture", CancellationToken::new())
        .await
        .unwrap();
    drop(c);
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
}
