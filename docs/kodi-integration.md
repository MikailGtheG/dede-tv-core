# Kodi playback integration contract — prototype

This module constructs typed Kodi JSON-RPC 2.0 commands. It **does not**
open a network connection or provide an unauthenticated HTTP service.

## PVR channels

`kodi::open_pvr_channel(channel_id, request_id)` generates
`Player.Open` with `item.channelid`; IDs must be obtained from Kodi PVR
listings, not fabricated by the browser.

## Virtual local channels

`VirtualChannels::current(channel_name, now_ms)` resolves the local file
and its timeline offset; `kodi::open_virtual_channel(..., approved_roots, id)`
generates `Player.Open` with `item.file` and a time-based
`options.resume` value. The file must exist, be a regular file, and
resolve within an explicitly approved media root. Canonical path
resolution rejects traversal and symlink escapes.

The current time-based resume mechanism must be tested with the exact
target Kodi build, file formats, hardware decoder and seek index. A
Kodi JSON-RPC success response is not proof playback actually started.

## Future transport

The next step is a dedicated controller with *strictly local* transport.
Kodi's unauthenticated TCP JSON-RPC interface must not be exposed to
the home network or Tailnet. The web app must call only authenticated
and allowlisted controller actions.

Player state reconciliation, retry limits, cancellation of stale opens,
channel ordering, and remote key mapping are not yet implemented.
