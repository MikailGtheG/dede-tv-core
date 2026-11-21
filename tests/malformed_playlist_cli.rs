//! Invalid administrative commands must not mutate an existing TV lineup.
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
fn malformed_multi_video_add_preserves_existing_playlist_and_selection() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("channels.sqlite");
    assert!(cli(&db, &["add", "News", "original.mp4", "1000"])
        .status
        .success());
    assert!(cli(&db, &["select", "virtual:News"]).status.success());

    // An incomplete second video must not partially replace the channel.
    let malformed = cli(
        &db,
        &["add", "News", "replacement.mp4", "1000", "incomplete.mp4"],
    );
    assert!(!malformed.status.success());
    assert!(malformed.stdout.is_empty());

    assert_eq!(cli(&db, &["list"]).stdout, b"News\n");
    assert_eq!(cli(&db, &["last"]).stdout, b"virtual:News\n");
    let current = cli(&db, &["now", "News"]);
    assert!(current.status.success());
    assert!(
        String::from_utf8_lossy(&current.stdout).starts_with("original.mp4 @ "),
        "{current:?}"
    );
}
