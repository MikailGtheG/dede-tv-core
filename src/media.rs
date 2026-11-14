//! Non-recursive, bounded media discovery for a user-approved local folder.
//! Duration extraction (ffprobe) is separate and not implemented here.
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const MAX_MEDIA_FILES: usize = 10_000;

pub fn has_video_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "mp4" | "mkv" | "mov" | "m4v" | "webm" | "avi" | "ts"
            )
        })
}

/// Only direct children of a trusted directory. Symlinks and subfolders
/// are excluded; a file-count cap avoids building unbounded playlists.
pub fn discover_media(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let dir = fs::canonicalize(directory)?;
    if !dir.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "media root is not a directory",
        ));
    }
    let mut videos = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue; // rejects symlinks and directories
        }
        let path = entry.path();
        // Kodi JSON-RPC accepts UTF-8 paths only. Do not list media that
        // cannot later be opened without lossy filename substitution.
        if path.to_str().is_none() || !has_video_extension(&path) {
            continue;
        }
        if videos.len() >= MAX_MEDIA_FILES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "media directory exceeds file-count limit",
            ));
        }
        videos.push(path);
    }
    videos.sort();
    Ok(videos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn discovers_only_video_files_in_selected_folder() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(dir.path().join("a.MKV"), b"a").unwrap();
        fs::write(dir.path().join("b.mp4"), b"b").unwrap();
        fs::write(dir.path().join("secret.txt"), b"secret").unwrap();
        fs::write(nested.join("nested.mp4"), b"n").unwrap();
        let found = discover_media(dir.path()).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].file_name().unwrap(), "a.MKV");
        assert_eq!(found[1].file_name().unwrap(), "b.mp4");
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlink_and_broken_symlink() {
        use std::os::unix::fs::symlink;
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let file = outside.path().join("outside.mkv");
        fs::write(&file, b"v").unwrap();
        symlink(&file, dir.path().join("linked.mkv")).unwrap();
        symlink("nonexistent.mkv", dir.path().join("broken.mkv")).unwrap();
        assert!(discover_media(dir.path()).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn skips_non_utf8_filenames_that_kodi_cannot_open() {
        use std::os::unix::ffi::OsStringExt;
        let dir = tempdir().unwrap();
        let invalid = std::ffi::OsString::from_vec(b"episode-\xff.mkv".to_vec());
        fs::write(dir.path().join(invalid), b"v").unwrap();
        fs::write(dir.path().join("valid.mkv"), b"v").unwrap();
        let found = discover_media(dir.path()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file_name().unwrap(), "valid.mkv");
    }

    #[test]
    fn rejects_non_directory() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("video.mp4");
        fs::write(&file, b"v").unwrap();
        assert_eq!(
            discover_media(&file).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
