//! Exercise the administrative CLI against a real temporary SQLite database.
//! No Kodi service or external network access is required.
use std::process::{Command, Output};
use tempfile::tempdir;

fn cli(db: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dede-tv-core"))
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .expect("CLI binary should launch")
}

#[test]
fn rejected_virtual_names_never_modify_channel_list_or_resume_selection() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("channels.sqlite");

    let added = cli(&db, &["add", "News", "news.mp4", "1000"]);
    assert!(added.status.success(), "{added:?}");
    let selected = cli(&db, &["select", "virtual:News"]);
    assert!(selected.status.success(), "{selected:?}");

    let too_long = "é".repeat(47);
    for invalid in ["pvr:1", "virtual:News", "bad\nname", too_long.as_str()] {
        let result = cli(&db, &["add", invalid, "clip.mp4", "1000"]);
        assert!(
            !result.status.success(),
            "invalid name accepted: {invalid:?}"
        );
    }

    let listed = cli(&db, &["list"]);
    assert!(listed.status.success(), "{listed:?}");
    assert_eq!(String::from_utf8(listed.stdout).unwrap(), "News\n");

    let last = cli(&db, &["last"]);
    assert!(last.status.success(), "{last:?}");
    assert_eq!(String::from_utf8(last.stdout).unwrap(), "virtual:News\n");
}

#[test]
fn selection_rejects_missing_or_unresumable_channels_without_changing_last() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("channels.sqlite");
    assert!(cli(&db, &["add", "News", "news.mp4", "1000"])
        .status
        .success());
    assert!(cli(&db, &["select", "News"]).status.success());

    for invalid in ["Missing", "virtual:Missing", "virtual:bad\nname", "pvr:0"] {
        let result = cli(&db, &["select", invalid]);
        assert!(
            !result.status.success(),
            "invalid selection accepted: {invalid:?}"
        );
    }
    let last = cli(&db, &["last"]);
    assert!(last.status.success(), "{last:?}");
    assert_eq!(String::from_utf8(last.stdout).unwrap(), "News\n");
}

#[test]
fn request_virtual_rejects_invalid_names_before_generating_kodi_json() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("channels.sqlite");
    assert!(cli(&db, &["add", "News", "news.mp4", "1000"])
        .status
        .success());

    for invalid in ["pvr:1", "virtual:News", "bad\nname"] {
        let output = cli(
            &db,
            &["request-virtual", invalid, dir.path().to_str().unwrap()],
        );
        assert!(
            !output.status.success(),
            "invalid preview accepted: {invalid:?}"
        );
        assert!(
            output.stdout.is_empty(),
            "invalid preview emitted Kodi JSON"
        );
    }

    let last = cli(&db, &["last"]);
    assert!(last.status.success(), "{last:?}");
    assert!(last.stdout.is_empty() || last.stdout == b"\n");
}

#[test]
fn invalid_video_durations_never_replace_an_existing_channel() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("channels.sqlite");
    assert!(cli(&db, &["add", "News", "original.mp4", "1000"])
        .status
        .success());
    assert!(cli(&db, &["select", "virtual:News"])
        .status
        .success());

    for duration in ["0", "-1", "+1", "not-a-number", "18446744073709551616"] {
        let result = cli(&db, &["add", "News", "replacement.mp4", duration]);
        assert!(!result.status.success(), "invalid duration accepted: {duration:?}");
        assert!(result.stdout.is_empty(), "failed add printed success output");
    }

    let listed = cli(&db, &["list"]);
    assert!(listed.status.success(), "{listed:?}");
    assert_eq!(listed.stdout, b"News\n");

    let current = cli(&db, &["now", "News"]);
    assert!(current.status.success(), "{current:?}");
    assert!(
        String::from_utf8_lossy(&current.stdout).starts_with("original.mp4 @ "),
        "invalid add replaced the existing playlist: {current:?}"
    );

    let last = cli(&db, &["last"]);
    assert!(last.status.success(), "{last:?}");
    assert_eq!(last.stdout, b"virtual:News\n");
}
