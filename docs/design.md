# Architecture and milestones

The host daemon is Rust. The browser player is JavaScript. A future Swift helper owns ScreenCaptureKit capture and macOS input translation, reporting permission failures explicitly.

Capture publishes immutable, bounded frame snapshots. The first adapter connects only to a loopback RFB endpoint using classic VNC authentication. It validates pixel layout and rectangle geometry before allocating or copying. Endpoint credentials stay in an owner-only configuration file.

One encoder serves a capture source. Encoder input and output are cancellable. H.264 access units and viewer queues have byte and count budgets. A lagging viewer is disconnected rather than retaining old frames. New viewers wait for a fresh keyframe; no unbounded replay history is stored.

The web service listens on loopback, checks the exact browser Origin and uses expiring HttpOnly cookies after token sign-in. One viewer owns control. Disconnect releases held keys and buttons. The last viewer stops its capture and encoder; setup must not resurrect a cancelled session.

Milestones are: protocol and cancellation tests; hardware encoder and browser correctness; a real remote VM test; responsiveness and resource measurements; native physical desktop capture; distribution hardening. Synthetic tests do not establish VM compatibility or WAN latency. Frame-copy and idle costs remain measurable design questions.
