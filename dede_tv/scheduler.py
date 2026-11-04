"""Persistent virtual TV channel schedules with zero background transcoders."""
from __future__ import annotations

import json
import random
import sqlite3
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

MEDIA_EXTENSIONS = frozenset({".mp4", ".mkv", ".mov", ".m4v", ".webm", ".avi", ".ts"})


@dataclass(frozen=True)
class Video:
    path: str
    duration_ms: int


@dataclass(frozen=True)
class Playback:
    path: str
    position_ms: int
    next_transition_ms: int


def scan_media(folder: Path, duration: Callable[[Path], int]) -> list[Video]:
    """Scan one directory without traversing symlinks or child directories."""
    folder = folder.resolve(strict=True)
    if not folder.is_dir():
        raise NotADirectoryError(folder)
    videos = []
    for path in sorted(folder.iterdir()):
        if path.is_symlink() or not path.is_file() or path.suffix.lower() not in MEDIA_EXTENSIONS:
            continue
        try:
            ms = duration(path)
        except (ValueError, OSError):
            continue
        if type(ms) is int and 0 < ms <= 86400000:
            videos.append(Video(str(path), ms))
    return videos


class VirtualChannels:
    """SQLite-backed persistent shuffle. Query time determines channel position."""

    def __init__(self, database: Path, *, rng: random.Random | None = None):
        self.db = sqlite3.connect(database)
        self.db.execute("PRAGMA busy_timeout=5000")
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("""CREATE TABLE IF NOT EXISTS channels (
            name TEXT PRIMARY KEY, playlist TEXT NOT NULL,
            cycle_start_ms INTEGER NOT NULL
        )""")
        self.rng = rng or random.SystemRandom()

    def close(self) -> None:
        self.db.close()

    def set_channel(self, name: str, videos: list[Video], *, now_ms: int | None = None) -> None:
        if not name or len(name) > 100 or any(ord(c) < 32 for c in name):
            raise ValueError("invalid channel name")
        if not videos or len({v.path for v in videos}) != len(videos):
            raise ValueError("channel requires distinct videos")
        if any(type(v.duration_ms) is not int or v.duration_ms <= 0 for v in videos):
            raise ValueError("invalid video duration")
        shuffled = videos.copy()
        self.rng.shuffle(shuffled)
        now = int(time.time() * 1000) if now_ms is None else now_ms
        payload = json.dumps([v.__dict__ for v in shuffled], ensure_ascii=False)
        with self.db:
            self.db.execute("""INSERT INTO channels(name, playlist, cycle_start_ms)
                VALUES(?, ?, ?) ON CONFLICT(name) DO UPDATE SET
                playlist=excluded.playlist, cycle_start_ms=excluded.cycle_start_ms""",
                (name, payload, now))

    def current(self, name: str, *, now_ms: int | None = None) -> Playback:
        now = int(time.time() * 1000) if now_ms is None else now_ms
        self.db.execute("BEGIN IMMEDIATE")
        try:
            row = self.db.execute(
                "SELECT playlist, cycle_start_ms FROM channels WHERE name=?", (name,)
            ).fetchone()
            if row is None:
                raise KeyError(name)
            videos = [Video(**v) for v in json.loads(row[0])]
            start = row[1]
            elapsed = max(0, now - start)
            total = sum(v.duration_ms for v in videos)
            if not videos or total <= 0:
                raise ValueError("invalid stored playlist")
            if elapsed >= total:
                # Advance cycles without decoding background channels.
                # One reshuffle per completed cycle ensures all videos air once.
                while elapsed >= total:
                    previous_last = videos[-1].path
                    elapsed -= total
                    start += total
                    if len(videos) > 1:
                        self.rng.shuffle(videos)
                        if videos[0].path == previous_last:
                            index = next(i for i, v in enumerate(videos) if v.path != previous_last)
                            videos[0], videos[index] = videos[index], videos[0]
                self.db.execute(
                    "UPDATE channels SET playlist=?, cycle_start_ms=? WHERE name=?",
                    (json.dumps([v.__dict__ for v in videos], ensure_ascii=False), start, name)
                )
            offset = elapsed
            for video in videos:
                if offset < video.duration_ms:
                    playback = Playback(video.path, offset, now + video.duration_ms - offset)
                    self.db.commit()
                    return playback
                offset -= video.duration_ms
            raise AssertionError("invalid timeline position")
        except BaseException:
            self.db.rollback()
            raise
