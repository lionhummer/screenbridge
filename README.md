# Screenbridge

A Rust host service for experimental macOS desktop streaming. The first milestone reads a VM's loopback RFB framebuffer, encodes H.264 with VideoToolbox through FFmpeg, and serves an authenticated browser player. Native desktop capture will use a small ScreenCaptureKit helper.

This is a fresh implementation under development. It is not a stable release and does not yet establish real-VM or internet performance. Read [design](docs/design.md) for boundaries and milestones.

## Development

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

The service is intended to ship as one Apple Silicon macOS executable with an external FFmpeg dependency. It will not require Node, Bun, or a guest agent. Physical desktop capture and control require native macOS permissions and are a separate adapter milestone.

MIT OR Apache-2.0.
