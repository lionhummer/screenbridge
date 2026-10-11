# Installing the Rust prototype

Screenbridge builds into a single host executable. The browser assets are embedded. A deployed Mac needs no Rust toolchain, JavaScript runtime or guest agent. FFmpeg remains a separate executable so codec and licensing boundaries are explicit.

## Build and verify

On Apple Silicon macOS with Rust 1.85+ and Xcode Command Line Tools:

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
./scripts/build.sh
```

Use a trusted FFmpeg with VideoToolbox, or build a minimal non-GPL dependency from pinned upstream source:

```sh
./scripts/build-ffmpeg.sh
SCREENBRIDGE_FFMPEG="$PWD/artifacts/ffmpeg/bin/ffmpeg" cargo test --test encoder_roundtrip -- --ignored
SCREENBRIDGE_FFMPEG="$PWD/artifacts/ffmpeg/bin/ffmpeg" cargo test --test websocket_control -- --ignored
```

The source build retains the downloaded source, its SHA-256 and the LGPL license. Preserve corresponding source and build instructions when redistributing that dependency. The repository license does not replace FFmpeg's license.

## Install on the host

```sh
./scripts/install.sh artifacts
```

Or copy the executable and checksum directory to a remote host and run the installer there. Copy the `artifacts/ffmpeg` prefix too when using that dependency. Verify the executable checksum before starting. Installation creates user-owned directories; it does not register a service or enable network access.

## Configure

Create an owner-only JSON file (mode 600) in `~/.local/share/screenbridge/private/config.json`:

```json
{
  "vnc_port": 59001,
  "vnc_password": "REPLACE_WITH_CURRENT_VM_PASSWORD",
  "access_token": "REPLACE_WITH_A_RANDOM_32_BYTE_OR_LONGER_TOKEN",
  "ffmpeg": "/ABSOLUTE/INSTALL/PATH/ffmpeg/bin/ffmpeg",
  "listen_port": 8787,
  "public_origin": "https://YOUR_MAC.YOUR_TAILNET.ts.net:9443",
  "fps": 30,
  "bitrate_kbps": 6000
}
```

Generate a random token with `openssl rand -hex 32`; put it only in the private file and sign-in form. Do not pass secrets as command-line arguments. The server listens only on `127.0.0.1` and the RFB adapter connects only to loopback. For local testing, an exact HTTP loopback origin is accepted. Remote browsers require HTTPS for WebCodecs.

Start a disposable VM with a local RFB endpoint. With Tart, `tart run YOUR_VM --no-graphics --vnc-experimental` emits a per-run loopback port and password. Treat that output as secret. The endpoint can change on every boot. Stop Screenbridge, update its private config and restart when the endpoint changes. This repository does not provision VMs.

```sh
~/.local/share/screenbridge/bin/screenbridge --config ~/.local/share/screenbridge/private/config.json
```

SIGINT and SIGTERM stop capture and encoding. Begin with a manually managed test process; permanent service registration is a later milestone.

## Private internet test

Inspect existing Tailscale Serve configuration first. Pick an unused HTTPS port so other services remain available:

```sh
tailscale serve status
tailscale serve --bg --https=9443 http://127.0.0.1:8787
```

Set `public_origin` to the exact HTTPS origin, including the port. Sign in there with the token. Serve restricts access to the tailnet; public Funnel exposure is a separate decision. Stop only the matching test Serve listener afterwards, without resetting unrelated configuration.

The first viewer controls input. Other viewers are view-only. Disconnecting a controller releases held keys and pointer buttons before a replacement can take control. A newly connected viewer can acquire vacant control; existing viewers reconnect to request it. Cookies expire after one hour and restart invalidates them. WebSockets also have a one-hour lifetime.

Physical desktop capture requires a ScreenCaptureKit helper, a graphical session and Screen Recording permission; control additionally requires Accessibility permission. That adapter is planned and is not implemented in this prototype.
