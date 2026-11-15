//! Regression coverage for channel removal and boot-resume persistence.
use std::process::Command;
use tempfile::tempdir;

fn cli(db: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_dede-tv-core"))
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .expect("CLI must start")
}

#[test]
fn removing_other_channel_preserves_selected_pvr_and_failed_removal_is_noop() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("channels.sqlite");
    assert!(cli(&db, &["add", "News", "news.mp4", "1000"])
        .status
        .success());
    assert!(cli(&db, &["select", "pvr:7"]).status.success());

    let missing = cli(&db, &["remove", "Missing"]);
    assert!(!missing.status.success());
    assert!(missing.stdout.is_empty());
    assert_eq!(cli(&db, &["last"]).stdout, b"pvr:7\n");

    assert!(cli(&db, &["remove", "News"]).status.success());
    assert!(cli(&db, &["list"]).stdout.is_empty());
    assert_eq!(cli(&db, &["last"]).stdout, b"pvr:7\n");
}

#[test]
fn removing_selected_virtual_channel_clears_boot_resume_state() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("channels.sqlite");
    assert!(cli(&db, &["add", "News", "news.mp4", "1000"])
        .status
        .success());
    assert!(cli(&db, &["select", "virtual:News"]).status.success());
    assert!(cli(&db, &["remove", "News"]).status.success());
    assert_eq!(cli(&db, &["last"]).stdout, b"\n");

    let resumed = cli(&db, &["resume-last", dir.path().to_str().unwrap()]);
    assert!(resumed.status.success(), "{resumed:?}");
    assert_eq!(resumed.stdout, b"No last channel stored\n");
}

#[test]
fn removing_legacy_bare_selected_channel_clears_boot_resume_state() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("channels.sqlite");
    assert!(cli(&db, &["add", "News", "news.mp4", "1000"])
        .status
        .success());
    assert!(cli(&db, &["select", "News"]).status.success());
    assert_eq!(cli(&db, &["last"]).stdout, b"News\n");

    assert!(cli(&db, &["remove", "News"]).status.success());
    assert_eq!(cli(&db, &["last"]).stdout, b"\n");

    let resumed = cli(&db, &["resume-last", dir.path().to_str().unwrap()]);
    assert!(resumed.status.success(), "{resumed:?}");
    assert_eq!(resumed.stdout, b"No last channel stored\n");
}
