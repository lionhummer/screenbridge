mod common;
use screenbridge::{
    config::Config,
    encoder,
    rfb::{Input, PixelLayout},
};
use std::{process::Stdio, sync::Arc, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};
use tokio_util::sync::CancellationToken;
#[tokio::test]
#[ignore = "requires FFmpeg; VideoToolbox hardware on macOS, libx264 on Linux"]
async fn real_encoder_roundtrip_and_release() {
    let ffmpeg = std::env::var("SCREENBRIDGE_FFMPEG")
        .expect("set SCREENBRIDGE_FFMPEG to an absolute FFmpeg path");
    let (port, mut inputs, fixture) = common::fixture(PixelLayout::Bgra, false, false).await;
    let c = Config {
        vnc_port: port,
        vnc_password: "fixture".into(),
        access_token: "public-fixture-token-not-a-secret-12345".into(),
        ffmpeg: ffmpeg.clone().into(),
        listen_port: 8787,
        public_origin: "http://127.0.0.1:8787".into(),
        fps: 30,
        bitrate_kbps: 6000,
    };
    let cancel = CancellationToken::new();
    let stream = encoder::start(Arc::new(c), cancel.clone(), !cfg!(target_os = "macos"))
        .await
        .unwrap();
    let mut viewer = stream.video.subscribe();
    let key = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let frame = viewer.recv().await.unwrap();
            if frame.key {
                return frame;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(&key.packet[..4], &[0x53, 0x42, 1, 1]);
    assert!(key.init.contains("avc1."));
    let mut child = Command::new(&ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "h264",
            "-i",
            "pipe:0",
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(&key.packet[16..]).await.unwrap();
    drop(stdin);
    let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout.len(), 64 * 48 * 3);
    let at = (24 * 64 + 32) * 3;
    for (i, value) in [40_u8, 80, 160].iter().enumerate() {
        assert!(output.stdout[at + i].abs_diff(*value) < 25);
    }
    stream
        .input
        .send(Input::Key {
            keysym: 0xff0d,
            down: true,
        })
        .await
        .unwrap();
    let (done, ack) = tokio::sync::oneshot::channel();
    stream.input.send(Input::Release { done }).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), ack)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(inputs.recv().await.unwrap()[0], 1);
    assert_eq!(inputs.recv().await.unwrap()[0], 0);
    cancel.cancel();
    drop(stream);
    fixture.abort();
}
