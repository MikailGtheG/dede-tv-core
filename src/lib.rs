//! Dede TV virtual channels: deterministic schedule, no background transcoding.
//! Not a media player or network-facing server.
pub mod kodi;
pub mod media;
pub mod picker;
pub mod remote;
pub mod transport;
pub mod tv;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Slot {
    pub path: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

/// Upper bound for one EPG query, independent of the requested window.
pub const MAX_SCHEDULE_SLOTS: usize = 5000;

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
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}
impl From<serde_json::Error> for ScheduleError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}
pub type Result<T> = std::result::Result<T, ScheduleError>;

pub struct VirtualChannels {
    db: Connection,
}

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
            );",
        )?;
        Ok(Self { db })
    }

    /// Persist the selected live channel so startup can resume it.
    /// Values are logical channel identifiers; never use them as filesystem paths.
    pub fn set_last_channel(&self, channel: &str) -> Result<()> {
        if channel.trim().is_empty() || channel.len() > 100 || channel.chars().any(char::is_control)
        {
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
        let found = self
            .db
            .query_row(
                "SELECT value FROM settings WHERE key='last_channel'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found)
    }

    pub fn set_channel(&mut self, name: &str, videos: &[Video], now_ms: u64) -> Result<()> {
        // Reserve space for the `virtual:` resume prefix in settings (100 bytes).
        // A channel that cannot be resumed must not be accepted for creation.
        if name.trim().is_empty() || name.len() > 92 || name.chars().any(char::is_control) {
            return Err(ScheduleError::InvalidChannel);
        }
        if videos.is_empty()
            || videos.len() > crate::media::MAX_MEDIA_FILES
            || videos
                .iter()
                .any(|v| v.path.is_empty() || v.path.len() > 4096 || v.duration_ms == 0)
            || videos
                .iter()
                .map(|v| &v.path)
                .collect::<std::collections::HashSet<_>>()
                .len()
                != videos.len()
            || videos
                .iter()
                .try_fold(0u64, |a, v| a.checked_add(v.duration_ms))
                .is_none()
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

    /// Return local channel names in a stable order for the TV channel picker.
    pub fn list_channels(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .db
            .prepare("SELECT name FROM channels ORDER BY name COLLATE NOCASE, name")?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(names)
    }

    /// Remove a local channel and clear its remembered selection atomically.
    pub fn remove_channel(&mut self, name: &str) -> Result<bool> {
        let tx = self.db.transaction()?;
        let changed = tx.execute("DELETE FROM channels WHERE name=?1", [name])?;
        if changed != 0 {
            tx.execute(
                "DELETE FROM settings WHERE key='last_channel' AND (value=?1 OR value=?2)",
                rusqlite::params![name, format!("virtual:{name}")],
            )?;
        }
        tx.commit()?;
        Ok(changed != 0)
    }

    pub fn current(&self, name: &str, now_ms: u64) -> Result<Playback> {
        let (videos, start_ms, seed, total) = self.load(name)?;
        if now_ms > i64::MAX as u64 {
            return Err(ScheduleError::InvalidPlaylist);
        }
        let elapsed = now_ms.saturating_sub(start_ms);
        let cycle = elapsed / total;
        let mut position = elapsed % total;
        let playlist = shuffle_cycle(&videos, seed, cycle);
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

    /// Programme items overlapping `[from_ms, until_ms)`, for the EPG.
    /// Returns at most `MAX_SCHEDULE_SLOTS` items.
    pub fn schedule(&self, name: &str, from_ms: u64, until_ms: u64) -> Result<Vec<Slot>> {
        let (videos, start_ms, seed, total) = self.load(name)?;
        if from_ms > until_ms || until_ms > i64::MAX as u64 {
            return Err(ScheduleError::InvalidPlaylist);
        }
        let from_ms = from_ms.max(start_ms);
        let mut slots = Vec::new();
        if from_ms >= until_ms {
            return Ok(slots);
        }
        let mut cycle = (from_ms - start_ms) / total;
        let mut cycle_start = start_ms + cycle * total; // <= from_ms
        while cycle_start < until_ms && slots.len() < MAX_SCHEDULE_SLOTS {
            let mut item_start = cycle_start;
            for video in shuffle_cycle(&videos, seed, cycle) {
                let item_end = item_start.saturating_add(video.duration_ms);
                if item_end > from_ms && item_start < until_ms {
                    slots.push(Slot {
                        path: video.path,
                        start_ms: item_start,
                        end_ms: item_end,
                    });
                    if slots.len() == MAX_SCHEDULE_SLOTS {
                        break;
                    }
                }
                item_start = item_end;
            }
            cycle += 1;
            cycle_start = cycle_start.saturating_add(total);
        }
        Ok(slots)
    }

    fn load(&self, name: &str) -> Result<(Vec<Video>, u64, u64, u64)> {
        let row: Option<(String, i64, i64)> = self
            .db
            .query_row(
                "SELECT playlist, cycle_start_ms, shuffle_seed FROM channels WHERE name=?1",
                [name],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let (payload, start_ms, seed) = row.ok_or(ScheduleError::NotFound)?;
        let videos: Vec<Video> = serde_json::from_str(&payload)?;
        // Revalidate persisted playlists: SQLite can be edited externally.
        // Duplicate paths can violate shuffle_cycle's uniqueness invariant.
        if videos.is_empty()
            || videos.len() > crate::media::MAX_MEDIA_FILES
            || videos.iter().any(|video| {
                video.path.is_empty() || video.path.len() > 4096 || video.duration_ms == 0
            })
            || videos
                .iter()
                .map(|video| &video.path)
                .collect::<std::collections::HashSet<_>>()
                .len()
                != videos.len()
            || start_ms < 0
            || seed < 0
        {
            return Err(ScheduleError::InvalidPlaylist);
        }
        let total = videos
            .iter()
            .try_fold(0u64, |a, v| a.checked_add(v.duration_ms))
            .filter(|n| *n > 0)
            .ok_or(ScheduleError::InvalidPlaylist)?;
        Ok((videos, start_ms as u64, seed as u64, total))
    }
}

fn permutation(videos: &[Video], seed: u64, cycle: u64) -> Vec<Video> {
    let mut list = videos.to_vec();
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ cycle.wrapping_mul(0x9E3779B97F4A7C15));
    list.shuffle(&mut rng);
    list
}

fn shuffle_cycle(videos: &[Video], seed: u64, cycle: u64) -> Vec<Video> {
    if videos.len() == 1 {
        return videos.to_vec();
    }
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
        Video {
            path: name.to_owned(),
            duration_ms: ms,
        }
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
        assert!(matches!(
            s.set_last_channel("bad\nname"),
            Err(ScheduleError::InvalidChannel)
        ));
        assert!(matches!(
            s.set_last_channel("   "),
            Err(ScheduleError::InvalidChannel)
        ));
        assert_eq!(s.last_channel().unwrap().as_deref(), Some("TRT 1"));
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
        s.set_channel("A", &[v("a", 100), v("b", 100), v("c", 100)], 0)
            .unwrap();
        for cycle in 1..200 {
            assert_ne!(
                s.current("A", cycle * 300 - 1).unwrap().path,
                s.current("A", cycle * 300).unwrap().path
            );
        }
    }
    #[test]
    fn handles_many_cycles_without_looping_over_each_cycle() {
        let mut s = VirtualChannels::open(":memory:").unwrap();
        s.set_channel("A", &[v("a", 100), v("b", 100), v("c", 100)], 0)
            .unwrap();
        assert!(s.current("A", 9_000_000_000).is_ok());
    }
    #[test]
    fn rejects_oversized_playlists_before_persisting() {
        let mut s = VirtualChannels::open(":memory:").unwrap();
        let oversized = (0..=crate::media::MAX_MEDIA_FILES)
            .map(|i| v(&format!("{i}.mkv"), 1000))
            .collect::<Vec<_>>();
        assert!(matches!(
            s.set_channel("A", &oversized, 0),
            Err(ScheduleError::InvalidPlaylist)
        ));
        assert!(s.list_channels().unwrap().is_empty());
        assert!(matches!(
            s.set_channel("A", &[v(&"a".repeat(4097), 1000)], 0),
            Err(ScheduleError::InvalidPlaylist)
        ));
    }
    #[test]
    fn rejects_invalid_channel_data() {
        let mut s = VirtualChannels::open(":memory:").unwrap();
        assert!(matches!(
            s.set_channel("", &[v("a", 100)], 0),
            Err(ScheduleError::InvalidChannel)
        ));
        for invalid in ["   ", "bad\nname", &"x".repeat(93)] {
            assert!(matches!(
                s.set_channel(invalid, &[v("a", 100)], 0),
                Err(ScheduleError::InvalidChannel)
            ));
        }
        // Every accepted name must fit its persisted resume identity.
        let longest = "x".repeat(92);
        s.set_channel(&longest, &[v("a", 100)], 0).unwrap();
        s.set_last_channel(&format!("virtual:{longest}")).unwrap();
        assert!(matches!(
            s.set_channel("A", &[], 0),
            Err(ScheduleError::InvalidPlaylist)
        ));
        assert!(matches!(
            s.set_channel("A", &[v("a", 100), v("a", 100)], 0),
            Err(ScheduleError::InvalidPlaylist)
        ));
    }
    #[test]
    fn channel_listing_and_removal_persist() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("channels.sqlite");
        let mut s = VirtualChannels::open(&db).unwrap();
        assert!(s.list_channels().unwrap().is_empty());
        s.set_channel("ZDF", &[v("z.mkv", 1000)], 0).unwrap();
        s.set_channel("Dede Film", &[v("d.mkv", 1000)], 0).unwrap();
        s.set_last_channel("virtual:Dede Film").unwrap();
        assert_eq!(s.list_channels().unwrap(), vec!["Dede Film", "ZDF"]);
        assert!(!s.remove_channel("not-found").unwrap());
        assert!(s.remove_channel("Dede Film").unwrap());
        assert_eq!(s.last_channel().unwrap(), None);
        assert!(matches!(
            s.current("Dede Film", 1),
            Err(ScheduleError::NotFound)
        ));
        drop(s);
        let s = VirtualChannels::open(&db).unwrap();
        assert_eq!(s.list_channels().unwrap(), vec!["ZDF"]);
    }

    #[test]
    fn corrupted_zero_duration_media_is_rejected_before_scheduling() {
        let mut s = VirtualChannels::open(":memory:").unwrap();
        s.set_channel("Movies", &[v("valid.mp4", 1000)], 0).unwrap();
        // Simulate a damaged or externally modified database row. The
        // scheduler must not treat a zero-length entry as playable media.
        let corrupted =
            serde_json::to_string(&vec![v("zero.mp4", 0), v("valid.mp4", 1000)]).unwrap();
        s.db.execute(
            "UPDATE channels SET playlist=?1 WHERE name='Movies'",
            [corrupted],
        )
        .unwrap();
        assert!(matches!(
            s.current("Movies", 500),
            Err(ScheduleError::InvalidPlaylist)
        ));
    }

    #[test]
    fn corrupted_duplicate_media_paths_are_rejected_before_shuffle() {
        let mut s = VirtualChannels::open(":memory:").unwrap();
        s.set_channel(
            "Movies",
            &[v("a.mp4", 100), v("b.mp4", 100), v("c.mp4", 100)],
            0,
        )
        .unwrap();
        let corrupted = serde_json::to_string(&vec![
            v("same.mp4", 100),
            v("same.mp4", 100),
            v("other.mp4", 100),
        ])
        .unwrap();
        s.db.execute(
            "UPDATE channels SET playlist=?1 WHERE name='Movies'",
            [corrupted],
        )
        .unwrap();
        assert!(matches!(
            s.current("Movies", 300),
            Err(ScheduleError::InvalidPlaylist)
        ));
    }

    #[test]
    fn handles_two_item_cycle_boundaries() {
        let mut s = VirtualChannels::open(":memory:").unwrap();
        s.set_channel("A", &[v("a", 100), v("b", 100)], 0).unwrap();
        for cycle in 1..100 {
            assert_ne!(
                s.current("A", cycle * 200 - 1).unwrap().path,
                s.current("A", cycle * 200).unwrap().path
            );
        }
    }

    #[test]
    fn schedule_matches_current_playback() {
        let mut s = VirtualChannels::open(":memory:").unwrap();
        let videos = [
            v("a.mp4", 700),
            v("b.mp4", 1100),
            v("c.mp4", 400),
            v("d.mp4", 900),
        ];
        s.set_channel("A", &videos, 1_000).unwrap();
        let slots = s.schedule("A", 5_000, 20_000).unwrap();
        assert!(slots[0].start_ms <= 5_000 && slots[0].end_ms > 5_000);
        assert!(slots.last().unwrap().end_ms >= 20_000);
        for pair in slots.windows(2) {
            assert_eq!(pair[0].end_ms, pair[1].start_ms);
        }
        for slot in &slots {
            for t in [slot.start_ms.max(5_000), (slot.start_ms + slot.end_ms) / 2] {
                let play = s.current("A", t).unwrap();
                assert_eq!(play.path, slot.path);
                assert_eq!(play.position_ms, t - slot.start_ms);
            }
        }
    }

    #[test]
    fn schedule_starts_at_channel_creation_and_is_bounded() {
        let mut s = VirtualChannels::open(":memory:").unwrap();
        s.set_channel("A", &[v("a.mp4", 1), v("b.mp4", 1)], 500)
            .unwrap();
        let slots = s.schedule("A", 0, 510).unwrap();
        assert_eq!(slots.first().unwrap().start_ms, 500);
        assert_eq!(slots.len(), 10);
        let many = s.schedule("A", 0, u32::MAX as u64).unwrap();
        assert_eq!(many.len(), MAX_SCHEDULE_SLOTS);
        assert!(s.schedule("A", 0, 400).unwrap().is_empty());
        assert!(s.schedule("A", 600, 500).is_err());
        assert!(matches!(
            s.schedule("B", 0, 1),
            Err(ScheduleError::NotFound)
        ));
    }
}
