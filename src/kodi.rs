//! Pure Kodi JSON-RPC request construction. Network transport is intentionally absent:
//! Kodi must not expose unauthenticated JSON-RPC to the LAN or internet.
use crate::Playback;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum KodiError {
    NoApprovedMediaRoot,
    OutsideApprovedMediaRoot,
    InvalidMediaPath(std::io::Error),
}

fn approved_media_path(file: &str, roots: &[PathBuf]) -> Result<PathBuf, KodiError> {
    if roots.is_empty() {
        return Err(KodiError::NoApprovedMediaRoot);
    }
    let path = Path::new(file);
    if !path.is_absolute() {
        return Err(KodiError::OutsideApprovedMediaRoot);
    }
    let canonical = fs::canonicalize(path).map_err(KodiError::InvalidMediaPath)?;
    if !canonical.is_file() {
        return Err(KodiError::OutsideApprovedMediaRoot);
    }
    let is_allowed = roots.iter().any(|root| {
        fs::canonicalize(root)
            .map(|safe_root| canonical.starts_with(&safe_root) && canonical != safe_root)
            .unwrap_or(false)
    });
    if !is_allowed {
        return Err(KodiError::OutsideApprovedMediaRoot);
    }
    Ok(canonical)
}

/// JSON-RPC Player.Open request with an explicit resume position.
///
/// The caller must route this request through a separate restricted/authenticated
/// local transport. Never enable Kodi JSON-RPC TCP on an externally reachable NIC.
pub fn open_virtual_channel(
    playback: &Playback,
    approved_roots: &[PathBuf],
    request_id: u64,
) -> Result<Value, KodiError> {
    let file = approved_media_path(&playback.path, approved_roots)?;
    let milliseconds = playback.position_ms;
    // Kodi's Player.Position.Time uses hours, minutes, seconds, milliseconds.
    let resume = json!({
        "hours": milliseconds / 3_600_000,
        "minutes": (milliseconds / 60_000) % 60,
        "seconds": (milliseconds / 1_000) % 60,
        "milliseconds": milliseconds % 1_000
    });
    Ok(json!({
        "jsonrpc": "2.0",
        "method": "Player.Open",
        "params": {
            "item": { "file": file.to_string_lossy() },
            "options": { "resume": resume }
        },
        "id": request_id
    }))
}

/// Open an existing Kodi PVR channel. This sends no arbitrary URL or file path.
pub fn open_pvr_channel(channel_id: u32, request_id: u64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "Player.Open",
        "params": { "item": { "channelid": channel_id } },
        "id": request_id
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn pvr_request_uses_channel_identifier() {
        let r = open_pvr_channel(42, 8);
        assert_eq!(r["params"]["item"]["channelid"], 42);
        assert_eq!(r["method"], "Player.Open");
    }

    #[test]
    fn local_media_request_preserves_seek_position() {
        let dir = tempdir().unwrap();
        let movie = dir.path().join("Folge 001.mkv");
        fs::write(&movie, b"placeholder").unwrap();
        let playback = Playback {
            path: movie.to_string_lossy().to_string(),
            position_ms: 3_726_123,
            next_transition_ms: 0,
        };
        let r = open_virtual_channel(&playback, &[dir.path().to_path_buf()], 5).unwrap();
        assert_eq!(r["params"]["options"]["resume"]["hours"], 1);
        assert_eq!(r["params"]["options"]["resume"]["minutes"], 2);
        assert_eq!(r["params"]["options"]["resume"]["seconds"], 6);
        assert_eq!(r["params"]["options"]["resume"]["milliseconds"], 123);
        assert_eq!(r["params"]["item"]["file"], movie.to_string_lossy().as_ref());
    }

    #[test]
    fn rejects_unapproved_path_and_symlink_escape() {
        let allowed = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let foreign = outside.path().join("outside.mkv");
        fs::write(&foreign, b"placeholder").unwrap();
        let item = Playback {
            path: foreign.to_string_lossy().to_string(),
            position_ms: 0,
            next_transition_ms: 0,
        };
        assert!(matches!(
            open_virtual_channel(&item, &[allowed.path().to_path_buf()], 1),
            Err(KodiError::OutsideApprovedMediaRoot)
        ));
        #[cfg(unix)]
        {
            let symlink = allowed.path().join("escape.mkv");
            std::os::unix::fs::symlink(&foreign, &symlink).unwrap();
            let escaped = Playback {
                path: symlink.to_string_lossy().to_string(),
                ..item
            };
            assert!(matches!(
                open_virtual_channel(&escaped, &[allowed.path().to_path_buf()], 1),
                Err(KodiError::OutsideApprovedMediaRoot)
            ));
        }
    }

    #[test]
    fn no_root_fails_closed() {
        let item = Playback { path: "/tmp/a.mkv".to_string(), position_ms: 0, next_transition_ms: 0 };
        assert!(matches!(
            open_virtual_channel(&item, &[], 1),
            Err(KodiError::NoApprovedMediaRoot)
        ));
    }
}
