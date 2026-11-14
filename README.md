# Dede TV Core (Rust)

Native library for persistent virtual live-TV channel schedules; an early Dede TV OS component. **Prototype, not production-ready.**

## Implemented
- SQLite-backed channel settings (WAL, busy timeout).
- Deterministic per-cycle pseudorandom video order.
- Playback position based on wall-clock time; no background players or transcoding.
- Bounded lookup independent of the number of elapsed cycles.
- Unit tests for persistence, timelines, rollover, path security, media scanning and input validation.
- Safe nonrecursive local media discovery (no symlinks, 10,000-file cap).
- Typed Kodi JSON-RPC request construction for PVR and local-file playback.
- Bounded **loopback-only** TCP transport for `Player.Open`, with a 3-second I/O timeout and response-ID checks.
- Native command-line tool to create channels, select/restore last channel and request/trigger Kodi playback.
- Last selected channel persists across service restarts.

## Development
```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

The GitHub Actions workflow also runs these commands. No Python runtime is required by this component.

## Not implemented
Target-hardware Kodi integration tests; playback health verification; media duration extraction; filesystem watchers; authenticated web API; hardware deployment. Never expose an unauthenticated management API on a network. Review all media roots, input caps and update signatures before shipping.

## Native CLI smoke example (development only)

```sh
cargo run -- --db /tmp/dede-test.sqlite add "Dede Film" /media/film.mkv 3600000
cargo run -- --db /tmp/dede-test.sqlite select "Dede Film"
cargo run -- --db /tmp/dede-test.sqlite last
cargo run -- --db /tmp/dede-test.sqlite now "Dede Film"
cargo run -- --db /tmp/dede-test.sqlite request-pvr 12
# With Kodi's TCP JSON-RPC enabled *on loopback only*:
cargo run -- --db /tmp/dede-test.sqlite play-pvr 12
```

The transport is deliberately restricted to 127.0.0.1:9090. The CLI does
not authenticate users: **only an administrator with local permissions may
run it**. Never expose Kodi's unauthenticated JSON-RPC endpoint publicly.
A successful `Player.Open` JSON reply does not prove that video decoding
has started; end-to-end playback tests remain essential.
