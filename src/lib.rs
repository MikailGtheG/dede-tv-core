//! Dede TV virtual channels: deterministic schedule, no background transcoding.
//! Not a media player or network-facing server.
pub mod kodi;
pub mod media;
use rand::{seq::SliceRandom, Rng};
use rand_chacha::{rand_core::SeedableRng, ChaCha8Rng};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Video {
    pub path: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playback {
    pub path: String,
    pub position_ms: u64,
    pub next_transition_ms: u64,
}

#[derive(Debug)]
pub enum ScheduleError {
    InvalidChannel,
    InvalidPlaylist,
    NotFound,
    Database(rusqlite::Error),
    Serialization(serde_json::Error),
}
impl From<rusqlite::Error> for ScheduleError {
    fn from(error: rusqlite::Error) -> Self { Self::Database(error) }
}
impl From<serde_json::Error> for ScheduleError {
    fn from(error: serde_json::Error) -> Self { Self::Serialization(error) }
}
pub type Result<T> = std::result::Result<T, ScheduleError>;

pub struct VirtualChannels { db: Connection }

impl VirtualChannels {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS channels (
                name TEXT PRIMARY KEY,
                playlist TEXT NOT NULL,
                cycle_start_ms INTEGER NOT NULL,
                shuffle_seed INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );"
        )?;
        Ok(Self { db })
    }

    /// Persist the selected live channel so startup can resume it.
    /// Values are logical channel identifiers; never use them as filesystem paths.
    pub fn set_last_channel(&self, channel: &str) -> Result<()> {
        if channel.is_empty() || channel.len() > 100 || channel.chars().any(char::is_control) {
            return Err(ScheduleError::InvalidChannel);
        }
        self.db.execute(
            "INSERT INTO settings(key,value) VALUES('last_channel',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [channel],
        )?;
        Ok(())
    }

    /// Returns None on fresh installation; startup decides its default channel.
    pub fn last_channel(&self) -> Result<Option<String>> {
        let found = self.db.query_row(
            "SELECT value FROM settings WHERE key='last_channel'",
            [],
            |row| row.get(0),
        ).optional()?;
        Ok(found)
    }

    pub fn set_channel(&mut self, name: &str, videos: &[Video], now_ms: u64) -> Result<()> {
        if name.is_empty() || name.len() > 100 || name.chars().any(char::is_control) {
            return Err(ScheduleError::InvalidChannel);
        }
        if videos.is_empty() || videos.iter().any(|v| v.path.is_empty() || v.duration_ms == 0)
            || videos.iter().map(|v| &v.path).collect::<std::collections::HashSet<_>>().len() != videos.len()
            || videos.iter().try_fold(0u64, |a, v| a.checked_add(v.duration_ms)).is_none()
            || now_ms > i64::MAX as u64
        {
            return Err(ScheduleError::InvalidPlaylist);
        }
        let seed: i64 = rand::thread_rng().gen_range(0..=i64::MAX);
        let payload = serde_json::to_string(videos)?;
        self.db.execute(
            "INSERT INTO channels (name, playlist, cycle_start_ms, shuffle_seed)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(name) DO UPDATE SET
             playlist=excluded.playlist, cycle_start_ms=excluded.cycle_start_ms,
             shuffle_seed=excluded.shuffle_seed",
            params![name, payload, now_ms as i64, seed],
        )?;
        Ok(())
    }

    pub fn current(&self, name: &str, now_ms: u64) -> Result<Playback> {
        let row: Option<(String, i64, i64)> = self.db.query_row(
            "SELECT playlist, cycle_start_ms, shuffle_seed FROM channels WHERE name=?1",
            [name],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;
        let (payload, start_ms, seed) = row.ok_or(ScheduleError::NotFound)?;
        let videos: Vec<Video> = serde_json::from_str(&payload)?;
        if videos.is_empty() || start_ms < 0 || seed < 0 || now_ms > i64::MAX as u64 {
            return Err(ScheduleError::InvalidPlaylist);
        }
        let total = videos.iter().try_fold(0u64, |a, v| a.checked_add(v.duration_ms))
            .filter(|n| *n > 0).ok_or(ScheduleError::InvalidPlaylist)?;
        let elapsed = now_ms.saturating_sub(start_ms as u64);
        let cycle = elapsed / total;
        let mut position = elapsed % total;
        let playlist = shuffle_cycle(&videos, seed as u64, cycle);
        for video in playlist {
            if position < video.duration_ms {
                return Ok(Playback {
                    path: video.path,
                    position_ms: position,
                    next_transition_ms: now_ms.saturating_add(video.duration_ms - position),
                });
            }
            position -= video.duration_ms;
        }
        Err(ScheduleError::InvalidPlaylist)
    }
}

fn permutation(videos: &[Video], seed: u64, cycle: u64) -> Vec<Video> {
    let mut list = videos.to_vec();
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ cycle.wrapping_mul(0x9E3779B97F4A7C15));
    list.shuffle(&mut rng);
    list
}

fn shuffle_cycle(videos: &[Video], seed: u64, cycle: u64) -> Vec<Video> {
    if videos.len() == 1 { return videos.to_vec(); }
    if videos.len() == 2 {
        // To prevent an immediate boundary repeat with two videos,
        // both cycles must keep the same deterministic pair order.
        return permutation(videos, seed, 0);
    }
    let mut current = permutation(videos, seed, cycle);
    if cycle > 0 {
        // The last element of the previous cycle is never modified by
        // boundary correction (the swap uses an interior element).
        let previous = permutation(videos, seed, cycle - 1);
        let previous_last = &previous[previous.len() - 1].path;
        if current[0].path == *previous_last {
            let middle = (1..current.len() - 1)
                .find(|&i| current[i].path != *previous_last)
                .expect("unique media paths");
            current.swap(0, middle);
        }
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn v(name: &str, ms: u64) -> Video {
        Video { path: name.to_owned(), duration_ms: ms }
    }
    #[test]
    fn remembers_last_selected_channel_after_restart() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("last.sqlite");
        let s = VirtualChannels::open(&db).unwrap();
        assert_eq!(s.last_channel().unwrap(), None);
        s.set_last_channel("TRT 1").unwrap();
        drop(s);
        let s = VirtualChannels::open(&db).unwrap();
        assert_eq!(s.last_channel().unwrap().as_deref(), Some("TRT 1"));
        assert!(matches!(s.set_last_channel("bad\nname"), Err(ScheduleError::InvalidChannel)));
    }

    #[test]
    fn position_advances_while_tuned_away() {
        let mut s = VirtualChannels::open(":memory:").unwrap();
        s.set_channel("A", &[v("a.mp4", 1000)], 100).unwrap();
        assert_eq!(s.current("A", 350).unwrap().position_ms, 250);
        assert_eq!(s.current("A", 900).unwrap().position_ms, 800);
    }
    #[test]
    fn persists_across_restart() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("test.sqlite");
        let mut s = VirtualChannels::open(&db).unwrap();
        s.set_channel("A", &[v("a", 1000)], 100).unwrap();
        drop(s);
        let s = VirtualChannels::open(&db).unwrap();
        assert_eq!(s.current("A", 1100).unwrap().position_ms, 0);
    }
    #[test]
    fn prevents_boundary_repeats_for_three_or_more() {
        let mut s = VirtualChannels::open(":memory:").unwrap();
        s.set_channel("A", &[v("a",100),v("b",100),v("c",100)], 0).unwrap();
        for cycle in 1..200 {
            assert_ne!(s.current("A", cycle*300-1).unwrap().path,
                       s.current("A", cycle*300).unwrap().path);
        }
    }
    #[test]
    fn handles_many_cycles_without_looping_over_each_cycle() {
        let mut s = VirtualChannels::open(":memory:").unwrap();
        s.set_channel("A", &[v("a",100),v("b",100),v("c",100)], 0).unwrap();
        assert!(s.current("A", 9_000_000_000).is_ok());
    }
    #[test]
    fn rejects_invalid_channel_data() {
        let mut s = VirtualChannels::open(":memory:").unwrap();
        assert!(matches!(s.set_channel("", &[v("a",100)], 0), Err(ScheduleError::InvalidChannel)));
        assert!(matches!(s.set_channel("A", &[], 0), Err(ScheduleError::InvalidPlaylist)));
        assert!(matches!(s.set_channel("A", &[v("a",100),v("a",100)], 0), Err(ScheduleError::InvalidPlaylist)));
    }
    #[test]
    fn handles_two_item_cycle_boundaries() {
        let mut s = VirtualChannels::open(":memory:").unwrap();
        s.set_channel("A", &[v("a",100),v("b",100)], 0).unwrap();
        for cycle in 1..100 {
            assert_ne!(s.current("A", cycle*200-1).unwrap().path,
                       s.current("A", cycle*200).unwrap().path);
        }
    }
}
