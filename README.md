# Dede TV Core (Rust)

Native library for persistent virtual live-TV channel schedules; an early Dede TV OS component. **Prototype, not production-ready.**

## Implemented
- SQLite-backed channel settings (WAL, busy timeout).
- Deterministic per-cycle pseudorandom video order.
- Playback position based on wall-clock time; no background players or transcoding.
- Bounded lookup independent of the number of elapsed cycles.
- Six unit tests for persistence, timelines, rollover and input validation.

## Development
```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

The GitHub Actions workflow also runs these commands. No Python runtime is required by this component.

## Not implemented
Kodi integration; media duration extraction; filesystem watchers; authenticated web API; hardware deployment. Never expose an unauthenticated management API on a network. Review all media roots, input caps and update signatures before shipping.
