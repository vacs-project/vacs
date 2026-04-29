//! Clip storage: directory layout, rolling deque, eviction, and exports.
//!
//! Directory layout under the configured root:
//! ```text
//! root/
//!   clip-{unix_ms}-{tap}.wav    rolling deque entries (evictable)
//!   saved/
//!     clip-{unix_ms}-{tap}-{freq?}-{callsign}.wav   exported, never evicted, frequency is omitted if the clip has no recorded one
//! ```
//!
//! Rolling clips are session-scoped: on [`ClipStore::open`] every `clip-*.wav` directly
//! under `root/` is deleted, since callsign/frequency metadata is held only in memory
//! and would be lost across restarts. Files in `saved/` are left alone.

use crate::replay::{ClipMeta, ReplayError, TapId};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use trackaudio::Frequency;

const CLIP_FILENAME_PREFIX: &str = "clip-";
const EXPORT_FILENAME_PREFIX: &str = "replay-";
const SAVED_DIR: &str = "saved";

/// Manages the on-disk clip store and the in-memory rolling deque.
#[derive(Debug)]
pub struct ClipStore {
    root: PathBuf,
    #[allow(
        dead_code,
        reason = "used by the deferred export() in the command layer"
    )]
    saved_dir: PathBuf,
    max_clips: usize,
    clips: VecDeque<ClipMeta>,
}

impl ClipStore {
    /// Open (or create) a store at `root`. Any pre-existing `clip-*.wav` files in the
    /// root directory are deleted, since their in-memory metadata is gone. Files under
    /// `saved/` and any unrelated files in `root/` are left untouched.
    pub fn open(root: PathBuf, max_clips: usize) -> Result<Self, ReplayError> {
        fs::create_dir_all(&root)?;
        let saved_dir = root.join(SAVED_DIR);
        fs::create_dir_all(&saved_dir)?;

        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with(CLIP_FILENAME_PREFIX) && name.ends_with(".wav") {
                let _ = fs::remove_file(&path);
            }
        }

        Ok(Self {
            root,
            saved_dir,
            max_clips,
            clips: VecDeque::new(),
        })
    }

    /// Reserve a path for a clip about to be written under `id`. The id is supplied by
    /// the caller (typically the FSM's monotonic clip counter) and embedded into the
    /// in-memory metadata once [`Self::commit`] is called. The clip is **not** added to
    /// the deque until commit. Filename collisions on `unix_ms+tap` fall back to a
    /// numeric suffix.
    pub fn allocate(&self, tap: TapId, started_at: SystemTime) -> PathBuf {
        let unix_ms = system_time_to_unix_ms(started_at);
        let base = format!("{CLIP_FILENAME_PREFIX}{unix_ms}-{}", tap.filename_token());
        unique_path(&self.root, &base, "wav")
    }

    /// Add a fully-written clip to the deque, evicting the oldest if over capacity.
    /// Returns any clips that were evicted.
    pub fn commit(&mut self, meta: ClipMeta) -> Vec<ClipMeta> {
        self.clips.push_back(meta);
        self.evict_overflow()
    }

    /// Discard a partially written clip (writer failed). Removes the file if present;
    /// the clip is **not** added to the deque.
    pub fn discard(&self, path: &Path) {
        let _ = fs::remove_file(path);
    }

    pub fn list(&self) -> Vec<ClipMeta> {
        self.clips.iter().rev().cloned().collect()
    }

    pub fn get(&self, id: u64) -> Option<ClipMeta> {
        self.clips.iter().find(|c| c.id == id).cloned()
    }

    /// Delete a clip from the deque and its file. Returns whether the clip existed.
    pub fn delete(&mut self, id: u64) -> Result<bool, ReplayError> {
        let Some(pos) = self.clips.iter().position(|c| c.id == id) else {
            return Ok(false);
        };
        let meta = self.clips.remove(pos).expect("position checked");
        if meta.path.exists() {
            fs::remove_file(&meta.path)?;
        }
        Ok(true)
    }

    /// Delete all rolling-deque clips. Files in `saved/` are left untouched.
    pub fn clear(&mut self) -> Result<(), ReplayError> {
        for meta in self.clips.drain(..) {
            if meta.path.exists() {
                let _ = fs::remove_file(&meta.path);
            }
        }
        Ok(())
    }

    /// Copy a clip to the saved directory (or `target_dir` if provided). Exported clips
    /// are exempt from eviction. Filename collisions are resolved by appending `-N`.
    ///
    /// Saved filenames are self-describing: `clip-{unix_ms}-{tap}-{freq?}-{callsign}.wav`,
    /// e.g. `clip-1745789012345-headset-121.500-DLH4AB.wav`. Frequency is omitted if the
    /// clip has no recorded frequency.
    pub fn export(&self, id: u64, target_dir: Option<&Path>) -> Result<PathBuf, ReplayError> {
        let meta = self
            .get(id)
            .ok_or_else(|| ReplayError::Wav(format!("clip {id} not found")))?;

        let dir = target_dir.unwrap_or(&self.saved_dir);
        fs::create_dir_all(dir)?;

        let unix_ms = system_time_to_unix_ms(meta.started_at);
        let callsign = meta.callsign.as_deref().unwrap_or("unknown");
        let safe_callsign = sanitize(callsign);
        let base = match meta.frequency {
            Some(freq) => format!(
                "{EXPORT_FILENAME_PREFIX}{}-{}-{}-{}",
                unix_ms,
                meta.tap.filename_token(),
                format_frequency_mhz(freq),
                safe_callsign
            ),
            None => format!(
                "{EXPORT_FILENAME_PREFIX}{}-{}-{}",
                unix_ms,
                meta.tap.filename_token(),
                safe_callsign
            ),
        };

        let target = unique_path(dir, &base, "wav");
        fs::copy(&meta.path, &target)?;

        Ok(target)
    }

    fn evict_overflow(&mut self) -> Vec<ClipMeta> {
        let mut evicted = Vec::new();
        while self.clips.len() > self.max_clips
            && let Some(meta) = self.clips.pop_front()
        {
            if meta.path.exists() {
                let _ = fs::remove_file(&meta.path);
            }
            evicted.push(meta);
        }
        evicted
    }
}

fn system_time_to_unix_ms(t: SystemTime) -> u128 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Format a frequency as `MHZ.kkk` (kHz precision), e.g. `121.500`. Hz below the kHz
/// boundary are truncated; this is what controllers expect to see in filenames.
#[allow(
    dead_code,
    reason = "used by the deferred export() in the command layer"
)]
fn format_frequency_mhz(freq: Frequency) -> String {
    let hz = u64::from(freq);
    let mhz = hz / 1_000_000;
    let khz = (hz % 1_000_000) / 1_000;
    format!("{mhz}.{khz:03}")
}

#[allow(
    dead_code,
    reason = "used by the deferred export() in the command layer"
)]
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn unique_path(dir: &Path, base: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{base}.{ext}"));
    if !first.exists() {
        return first;
    }
    for n in 1..u32::MAX {
        let candidate = dir.join(format!("{base}-{n}.{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    // This should be astronomically unlikely, but if we somehow exhaust the suffix space,
    // just return the first path and overwrite the existing file.
    dir.join(format!("{base}.{ext}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::writer::ClipWriter;
    use std::time::Duration;
    use tempfile::tempdir;

    fn write_dummy_clip(path: &Path) {
        let mut w = ClipWriter::create(path, 48_000, 1).unwrap();
        w.write_frame(&vec![0.0_f32; 480]).unwrap();
        w.finalize().unwrap();
    }

    fn fresh_meta(id: u64, path: PathBuf, tap: TapId) -> ClipMeta {
        ClipMeta {
            id,
            path,
            tap,
            callsign: Some("DLH123".into()),
            frequency: Some(Frequency::from(121_500_000_u64)),
            started_at: SystemTime::UNIX_EPOCH + Duration::from_millis(1_700_000_000_000),
            ended_at: SystemTime::UNIX_EPOCH + Duration::from_millis(1_700_000_001_000),
            duration_ms: 1_000,
        }
    }

    #[test]
    fn open_creates_root_and_saved_dirs() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("replay");
        let store = ClipStore::open(root.clone(), 10).unwrap();
        assert!(root.is_dir());
        assert!(root.join("saved").is_dir());
        assert!(store.list().is_empty());
    }

    #[test]
    fn allocate_uses_expected_filename() {
        let dir = tempdir().unwrap();
        let store = ClipStore::open(dir.path().to_path_buf(), 10).unwrap();
        let started = SystemTime::UNIX_EPOCH + Duration::from_millis(1_700_000_000_000);
        let path = store.allocate(TapId::Headset, started);
        let name = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(
            name,
            format!("{CLIP_FILENAME_PREFIX}1700000000000-headset.wav")
        );
    }

    #[test]
    fn allocate_resolves_filename_collisions() {
        let dir = tempdir().unwrap();
        let store = ClipStore::open(dir.path().to_path_buf(), 10).unwrap();
        let started = SystemTime::UNIX_EPOCH + Duration::from_millis(1_700_000_000_000);
        let p1 = store.allocate(TapId::Headset, started);
        write_dummy_clip(&p1);
        let p2 = store.allocate(TapId::Headset, started);
        assert_ne!(p1, p2);
        assert!(p2.file_name().unwrap().to_str().unwrap().contains("-1."));
    }

    #[test]
    fn commit_evicts_oldest_when_over_capacity() {
        let dir = tempdir().unwrap();
        let mut store = ClipStore::open(dir.path().to_path_buf(), 2).unwrap();
        let mut paths = Vec::new();
        for id in 1_u64..=3 {
            let path = store.allocate(TapId::Headset, SystemTime::now());
            write_dummy_clip(&path);
            paths.push(path.clone());
            let _ = store.commit(fresh_meta(id, path, TapId::Headset));
        }
        let listed = store.list();
        assert_eq!(listed.len(), 2);
        assert!(!paths[0].exists(), "oldest file should have been evicted");
        assert!(paths[1].exists());
        assert!(paths[2].exists());
    }

    #[test]
    fn delete_removes_file_and_entry() {
        let dir = tempdir().unwrap();
        let mut store = ClipStore::open(dir.path().to_path_buf(), 10).unwrap();
        let path = store.allocate(TapId::Speaker, SystemTime::now());
        write_dummy_clip(&path);
        let id = 1;
        store.commit(fresh_meta(id, path.clone(), TapId::Speaker));

        assert!(store.delete(id).unwrap());
        assert!(!path.exists());
        assert!(store.list().is_empty());
        assert!(!store.delete(id).unwrap());
    }

    #[test]
    fn clear_drops_all_rolling_clips_but_not_saved() {
        let dir = tempdir().unwrap();
        let mut store = ClipStore::open(dir.path().to_path_buf(), 10).unwrap();
        let path = store.allocate(TapId::Merged, SystemTime::now());
        write_dummy_clip(&path);
        let id = 1;
        store.commit(fresh_meta(id, path, TapId::Merged));
        let exported = store.export(id, None).unwrap();
        assert!(exported.exists());

        store.clear().unwrap();
        assert!(store.list().is_empty());
        assert!(exported.exists(), "saved clip must survive clear");
    }

    #[test]
    fn export_resolves_collisions() {
        let dir = tempdir().unwrap();
        let mut store = ClipStore::open(dir.path().to_path_buf(), 10).unwrap();
        let path = store.allocate(TapId::Headset, SystemTime::UNIX_EPOCH);
        write_dummy_clip(&path);
        let id = 1;
        store.commit(fresh_meta(id, path, TapId::Headset));

        let p1 = store.export(id, None).unwrap();
        let p2 = store.export(id, None).unwrap();
        let p3 = store.export(id, None).unwrap();
        assert_ne!(p1, p2);
        assert_ne!(p2, p3);
        assert!(p1.exists() && p2.exists() && p3.exists());
    }

    #[test]
    fn export_filename_includes_frequency_and_callsign() {
        let dir = tempdir().unwrap();
        let mut store = ClipStore::open(dir.path().to_path_buf(), 10).unwrap();
        let path = store.allocate(TapId::Headset, SystemTime::UNIX_EPOCH);
        write_dummy_clip(&path);
        let id = 1;
        store.commit(fresh_meta(id, path, TapId::Headset));

        let exported = store.export(id, None).unwrap();
        let name = exported.file_name().unwrap().to_str().unwrap();
        assert_eq!(
            name,
            format!("{EXPORT_FILENAME_PREFIX}1700000000000-headset-121.500-DLH123.wav")
        );
    }

    #[test]
    fn export_filename_omits_frequency_when_unknown() {
        let dir = tempdir().unwrap();
        let mut store = ClipStore::open(dir.path().to_path_buf(), 10).unwrap();
        let path = store.allocate(TapId::Speaker, SystemTime::UNIX_EPOCH);
        write_dummy_clip(&path);
        let id = 1;
        let mut meta = fresh_meta(id, path, TapId::Speaker);
        meta.frequency = None;
        store.commit(meta);

        let exported = store.export(id, None).unwrap();
        let name = exported.file_name().unwrap().to_str().unwrap();
        assert_eq!(
            name,
            format!("{EXPORT_FILENAME_PREFIX}1700000000000-speaker-DLH123.wav")
        );
    }

    #[test]
    fn format_frequency_mhz_pads_khz() {
        assert_eq!(
            format_frequency_mhz(Frequency::from(121_500_000_u64)),
            "121.500"
        );
        assert_eq!(
            format_frequency_mhz(Frequency::from(118_005_000_u64)),
            "118.005"
        );
        assert_eq!(
            format_frequency_mhz(Frequency::from(121_000_000_u64)),
            "121.000"
        );
    }

    #[test]
    fn open_purges_existing_rolling_clips_but_keeps_saved() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();

        // Seed root/ with two rolling clips and saved/ with one exported clip.
        let exported = {
            let mut store = ClipStore::open(root.clone(), 10).unwrap();
            for (id, ts) in [1_700_000_000_000_u64, 1_700_000_001_000_u64]
                .into_iter()
                .enumerate()
            {
                let started = SystemTime::UNIX_EPOCH + Duration::from_millis(ts);
                let path = store.allocate(TapId::Speaker, started);
                write_dummy_clip(&path);
                store.commit(fresh_meta(id as u64 + 1, path, TapId::Speaker));
            }
            let last_id = store.list().first().unwrap().id;
            store.export(last_id, None).unwrap()
        };
        // Drop two stray rolling clips on disk.
        let stray = root.join(format!("{CLIP_FILENAME_PREFIX}1700000005000-headset.wav"));
        write_dummy_clip(&stray);

        // Reopen: rolling clips in root/ are wiped, saved/ untouched.
        let store = ClipStore::open(root.clone(), 10).unwrap();
        assert!(store.list().is_empty());
        assert!(!stray.exists());
        assert!(exported.exists(), "saved clip must survive reopen");
    }

    #[test]
    fn open_leaves_unrelated_files_alone() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::write(root.join("notes.txt"), b"hello").unwrap();
        let _ = ClipStore::open(root.clone(), 10).unwrap();
        assert!(root.join("notes.txt").exists());
    }
}
