# Channel catalog and playback acknowledgement invariants

The TV controller currently emits a `TvEvent::Playback { channel, request }` but accepts a subsequent `confirm_playback(db, name)` by display name alone. The acknowledgement is **not** bound to a specific Kodi request, source identity, or catalog generation.

## Reproducible race

1. Build a catalog containing `("TRT 1", Source::Pvr(17))`.
2. Request playback of `TRT 1`; keep the returned `Player.Open` request pending.
3. Replace the catalog with `("TRT 1", Source::Pvr(18))`.
4. Acknowledge the original request with `confirm_playback(db, "TRT 1")`.
5. The current implementation can persist `pvr:18` even though Kodi acknowledged channel 17.

This is a correctness issue for resume and channel switching; the caller must not interpret the display name as an immutable playback identity.

## Required implementation follow-up

- Assign an opaque, monotonically increasing request token to each playback request, including the source identity and catalog generation.
- Accept acknowledgement only for the matching outstanding token and reject stale acknowledgements after catalog replacement or a newer tune request.
- Persist the source captured when the request was created, never a newly resolved display name.
- Add regression tests for same-name replacement, out-of-order Kodi acknowledgements, and channel removal while a request is pending.
- Preserve the existing rule that unsuccessful Kodi requests never update the last-channel state.

Until this is implemented and tested, integrations must not claim that asynchronous playback acknowledgements are race-safe.
