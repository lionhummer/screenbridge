# Evidence and performance plan

The Rust implementation has passed synthetic RFB capture/input tests and a real hardware-only VideoToolbox round trip on Apple Silicon macOS. The decoded pixels are checked against the fixture. A second integration test establishes exclusive controller ownership and key release over actual authenticated WebSocket connections.

An actual local browser also displayed the 64×48 synthetic fixture at 30 FPS, with a sampled receive-to-draw reading of roughly 0.4 ms and no decoder resets. This tiny fixture does not represent a full desktop workload.

Real 1280×800 VM video has also been encoded and decoded on an Apple Silicon host and received over verified private HTTPS on a separate Mac. Physical desktop capture and controlled internet latency measurements remain outstanding.

## Initial remote VM observation

On 2026-10-11, a live VM test exposed periodic canvas clearing when unchanged keyframe metadata arrived. The player now preserves the canvas and decoder for unchanged metadata, with regression tests for repeated metadata and actual geometry changes. FFmpeg now flushes every encoded packet to its output pipe.

Two short receiver samples used the same 30 FPS VM stream over a Tailscale relay. Each measurement ran for approximately 12 seconds, including the initial wait for a keyframe. Gaps below refer to binary video packet arrival times, not input latency.

| Sample | Received frames | Observed FPS | Median gap | 95th percentile gap | Maximum gap | Gaps under 2 ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Before explicit packet flushing | 321 | 31.2 | 2.0 ms | 116.1 ms | 1391.3 ms | 160 |
| After explicit packet flushing | 354 | 30.5 | 30.6 ms | 122.4 ms | 608.2 ms | 96 |

Encoder packet timestamps had a median spacing of 33.4 ms in both samples. The second sample had more regular median arrival spacing, but its tail remained poor. These uncontrolled samples cannot separate encoder buffering from changing relay conditions or establish a reliable latency improvement. Browser RTT observations varied from about 57 ms to above 200 ms; a short post-update playback observation showed zero decoder resets. Direct-path and sustained interactive tests are still needed.

## Measure the whole path

Use the same VM and resolution for an existing VNC baseline and Screenbridge. Include typing, text scrolling, window dragging, video, idle periods, multiple viewers, a slow receiver, resize and VM restart. Record CPU, resident memory, submitted and encoded frame counts, bitrate and reconnect behavior. The configured FPS is a target; use observed encoded and displayed counts for results.

The browser overlay reports displayed FPS, received bandwidth, ping RTT and receive-to-draw time. `/stats` provides submitted/encoded frames and encoded bytes after sign-in. Neither metric measures input-to-visible-response latency. Measure that independently with a timestamped visual feedback fixture or high-frame-rate recording.

Test from another network and record whether the tailnet connection is direct or relayed. A same-LAN connection, local tunnel or synthetic fixture cannot establish WAN performance. Take multiple runs and report distributions. Public results must contain only numeric results and generic environment details, never credentials, host endpoints or screen contents.

## Remaining costs

RFB keeps one mutable framebuffer and publishes an immutable snapshot after an update. Encoder ticks borrow the current snapshot rather than copying it per tick; updates still copy pixels. The feed remains fixed-rate, including an idle desktop. Encoded packets use shared byte buffers across viewers. The access-unit limit is 2 MiB and the broadcast ring holds eight packets; lagging receivers close rather than skipping dependent H.264 frames. Transport sends have a one-second deadline.

VideoToolbox uses explicit low-delay rate control, no B-frames and hardware-only encoding. Browser decoder behavior and actual emitted frame rate need verification on real workloads. TCP head-of-line blocking is another measurement question; evaluate WebRTC only after controlled WAN measurements.
