# Kodi loopback transport: security and reliability verification

Status: **test plan, not evidence of a vulnerability or completed test**.

## Boundary

The Rust core may send Kodi JSON-RPC playback commands to a loopback TCP listener. Loopback binding limits network exposure but **does not authenticate local processes**. A compromised local account or container sharing the host network namespace may be able to reach that listener. Do not bind Kodi JSON-RPC to 0.0.0.0 or expose it through Tailscale. Web administration must never proxy arbitrary JSON-RPC payloads.

## Required regression cases (implement as automated tests before release)

| ID | Input / setup | Expected result |
| --- | --- | --- |
| KLT-01 | Mock server returns matching JSON-RPC id, valid result | Success |
| KLT-02 | Server returns mismatched id | Reject response |
| KLT-03 | Server sends truncated JSON then closes | Fail without panic |
| KLT-04 | Server sends oversized JSON without newline | Fail at configured response-size bound |
| KLT-05 | Server accepts connection but never responds | Read timeout; no indefinite hang |
| KLT-06 | Server delays accepting / listener unavailable | Connection failure is bounded |
| KLT-07 | Server sends a notification before matching response | Do not accept notification as command acknowledgement |
| KLT-08 | Server returns JSON-RPC error for Player.Open | Surface failure; do not report playback success |
| KLT-09 | Concurrent playback commands | Ensure deterministic ordering or explicitly reject concurrency |
| KLT-10 | Symlink in approved media tree points outside root | Reject before issuing Player.Open |
| KLT-11 | Path replaced after validation, before playback | Document and mitigate TOCTOU risk; test race where feasible |
| KLT-12 | Kodi restarts during request | Bounded failure, retry policy must not duplicate unintended actions |

## Integration gates

1. Run `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --all-targets` with the pinned toolchain.
2. Capture test logs and GitHub Actions run URL for the exact head commit.
3. Verify real Kodi on Debian 13: cold boot opens last channel without home screen; PVR and local-video seek work; failure returns to a usable state.
4. Verify JSON-RPC binds only to 127.0.0.1/::1, that only intended local users can invoke control paths, and that web UI requires authentication and CSRF protections.
5. Record Kodi and pvr.iptvsimple upstream revisions before proposing a patch; reproduce every alleged defect with an isolated test.
6. Keep PR in draft until hardware tests, packaging provenance, and rollback checks are complete.

## Current evidence

GitHub Actions Rust core run 41988955196 reported success for commit `734125a2fee80687dd820d3d7f0c350cf4ba52e0`. This does **not** establish the outcomes of KLT-01 through KLT-12 or real hardware playback. See https://github.com/MikailGtheG/dede-tv-core/actions/runs/41988955196 .
