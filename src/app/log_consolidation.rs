//! Consolidation of STO's rotating combat logs into a single file (Linux).
//!
//! Under Proton, STO writes combat data to a new `combatlog_<timestamp>.log`
//! file every time it rotates the log (map change / relog / periodically),
//! instead of the single `combatlog.log` it writes on Windows. That scatters
//! combats across many files, breaking both browsing and the live overlay
//! (which follows one file).
//!
//! [`Consolidator`] rebuilds the Windows-like single `combatlog.log` in the
//! same folder: it appends every *complete* rotated file into it (then deletes
//! the original to avoid duplicating disk space) and mirrors the currently
//! *active* file's new bytes so it stays live. It runs in the background; the
//! analyzer reads whatever file the user configured (which may be
//! `combatlog.log` for the live / all-combats view, or a specific log to
//! review). A `protected` file passed to [`Consolidator::tick`] — the one being
//! read — is left untouched so opening a specific old log keeps working.
//!
//! Safety: the active file (still written by STO) is never deleted; a source is
//! deleted only after its bytes are appended, flushed and synced to disk, and
//! then read back from the merged file and compared byte-for-byte to the
//! source. Any failure rolls the merged file back to its previous length, so a
//! partial append is never left behind and a retry cannot duplicate data. The
//! mirror offset is persisted so a restart never re-appends the same bytes.
//! Verified that STO's rotated files do not duplicate each other's records, so
//! appending them cannot double-count damage.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The consolidated file, matching the name STO uses on Windows.
const TARGET_NAME: &str = "combatlog.log";
/// Sidecar remembering which active file we mirror and how far.
const STATE_NAME: &str = ".cla-consolidation";
/// Read-back chunk size when verifying appended bytes against the source.
const VERIFY_CHUNK: usize = 64 * 1024;

pub struct Consolidator {
    dir: PathBuf,
    target: PathBuf,
    /// Active (still-written) file currently mirrored, and how many of its
    /// bytes are already in `target`. Persisted across restarts.
    active: Option<PathBuf>,
    active_offset: u64,
}

impl Consolidator {
    pub fn new(dir: &Path) -> Self {
        let target = dir.join(TARGET_NAME);
        let mut consolidator = Self {
            dir: dir.to_path_buf(),
            target,
            active: None,
            active_offset: 0,
        };
        consolidator.load_state();
        consolidator
    }

    /// Runs one consolidation pass; logs and swallows I/O errors so a transient
    /// failure never takes down analysis. `protected` is a file currently being
    /// analyzed directly (e.g. an old log the user opened): it is left entirely
    /// untouched — neither merged nor deleted — so it stays available to read.
    pub fn tick(&mut self, protected: Option<&Path>) {
        if let Err(e) = self.run(protected) {
            log::warn!("combatlog consolidation: {e}");
        }
    }

    fn run(&mut self, protected: Option<&Path>) -> io::Result<()> {
        let rotated = self.rotated_files()?;
        if rotated.is_empty() {
            // Nothing to consolidate — e.g. on Windows, or before STO ran.
            return Ok(());
        }

        // The active file is the one STO still writes: most recently modified.
        let active = rotated
            .iter()
            .max_by_key(|(_, mtime)| *mtime)
            .map(|(path, _)| path.clone())
            .expect("rotated is non-empty");

        // Complete files: everything except the active one (and the protected
        // one being read), oldest first so the merged log stays in order.
        let mut complete: Vec<PathBuf> = rotated
            .into_iter()
            .map(|(path, _)| path)
            .filter(|path| path != &active && Some(path.as_path()) != protected)
            .collect();
        complete.sort_by_cached_key(|path| first_record_key(path));

        let mut target = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.target)?;

        for file in &complete {
            // If this was the file we had been mirroring, only its tail beyond
            // the mirror offset is still missing from the target.
            let from = if self.active.as_ref() == Some(file) {
                self.active_offset
            } else {
                0
            };
            append_verified(file, from, &mut target, &self.target)?;
            fs::remove_file(file)?;
        }

        // If the previously-mirrored active file was just merged & deleted,
        // drop that state before mirroring the new active file.
        if self
            .active
            .as_ref()
            .is_some_and(|prev| complete.contains(prev))
        {
            self.active = None;
            self.active_offset = 0;
        }

        // Mirror the active file's new bytes so the overlay follows it live.
        let from = if self.active.as_deref() == Some(active.as_path()) {
            self.active_offset
        } else {
            0
        };
        let new_offset = append_verified(&active, from, &mut target, &self.target)?;
        self.active = Some(active);
        self.active_offset = new_offset;
        self.save_state();
        Ok(())
    }

    fn rotated_files(&self) -> io::Result<Vec<(PathBuf, SystemTime)>> {
        let mut files = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // The consolidated target ("combatlog.log") has no '_', so the
            // "combatlog_" prefix excludes it from being consumed.
            if name.starts_with("combatlog_") && name.ends_with(".log") {
                let mtime = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                files.push((entry.path(), mtime));
            }
        }
        Ok(files)
    }

    fn state_path(&self) -> PathBuf {
        self.dir.join(STATE_NAME)
    }

    fn load_state(&mut self) {
        let Ok(content) = fs::read_to_string(self.state_path()) else {
            return;
        };
        let mut lines = content.lines();
        if let (Some(name), Some(offset)) = (lines.next(), lines.next()) {
            if !name.is_empty() {
                self.active = Some(self.dir.join(name));
                self.active_offset = offset.trim().parse().unwrap_or(0);
            }
        }
    }

    fn save_state(&self) {
        let name = self
            .active
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let content = format!("{}\n{}\n", name, self.active_offset);
        // Write + rename so a crash mid-write can't leave a torn state file.
        let tmp = self.dir.join(format!("{STATE_NAME}.tmp"));
        if fs::write(&tmp, content).is_ok() {
            let _ = fs::rename(&tmp, self.state_path());
        }
    }
}

/// Appends `src[from..]` to the consolidated `target`, then proves the bytes
/// landed before the caller trusts (and may delete) the source. Returns `src`'s
/// length so the caller can record the new mirror offset.
///
/// Safety: on ANY failure the target is rolled back to its pre-append length, so
/// a partial write is never left behind and a retry re-appends cleanly without
/// duplicating. On success the freshly appended range is read back from the
/// merged file and compared byte-for-byte to the source, so a source is only
/// ever deleted once its data is provably present in the merged file.
fn append_verified(
    src: &Path,
    from: u64,
    target: &mut File,
    target_path: &Path,
) -> io::Result<u64> {
    let base = target.metadata()?.len();
    match append_and_verify(src, from, target, target_path, base) {
        Ok(len) => Ok(len),
        Err(e) => {
            // Undo any partially written bytes so the next pass starts clean.
            let _ = target.flush();
            if let Err(rollback) = target.set_len(base).and_then(|()| target.sync_data()) {
                log::error!("combatlog consolidation: rollback after '{e}' failed: {rollback}");
            }
            Err(e)
        }
    }
}

fn append_and_verify(
    src: &Path,
    from: u64,
    target: &mut File,
    target_path: &Path,
    base: u64,
) -> io::Result<u64> {
    let mut source = File::open(src)?;
    let len = source.metadata()?.len();
    if from >= len {
        return Ok(len); // nothing new to copy
    }
    source.seek(SeekFrom::Start(from))?;
    let copied = io::copy(&mut source, target)?;
    target.flush()?;
    target.sync_data()?;
    if from + copied != len {
        return Err(io::Error::other(
            "short read while consolidating combat log",
        ));
    }
    if base + copied != target.metadata()?.len() {
        return Err(io::Error::other(
            "short write while consolidating combat log",
        ));
    }
    verify_range(src, from, target_path, base, copied)?;
    Ok(len)
}

/// Streams `target[base..base + count]` and `src[from..from + count]` and errors
/// unless every byte matches, so a mismatch stops the source from being deleted.
fn verify_range(
    src: &Path,
    from: u64,
    target_path: &Path,
    base: u64,
    count: u64,
) -> io::Result<()> {
    let mut source = File::open(src)?;
    source.seek(SeekFrom::Start(from))?;
    let mut merged = File::open(target_path)?;
    merged.seek(SeekFrom::Start(base))?;

    let mut source_buf = vec![0u8; VERIFY_CHUNK];
    let mut merged_buf = vec![0u8; VERIFY_CHUNK];
    let mut remaining = count;
    while remaining > 0 {
        let chunk = remaining.min(VERIFY_CHUNK as u64) as usize;
        source.read_exact(&mut source_buf[..chunk])?;
        merged.read_exact(&mut merged_buf[..chunk])?;
        if source_buf[..chunk] != merged_buf[..chunk] {
            return Err(io::Error::other(
                "merged combat log does not match source; keeping the original",
            ));
        }
        remaining -= chunk as u64;
    }
    Ok(())
}

/// Sort key for chronological ordering. STO records start with a fixed-width
/// `YY:MM:DD:HH:MM:SS.s` timestamp that sorts lexicographically in time order;
/// falls back to the file name when the first line can't be read.
fn first_record_key(path: &Path) -> String {
    if let Ok(mut file) = File::open(path) {
        let mut buf = [0u8; 24];
        if let Ok(read) = file.read(&mut buf) {
            if let Ok(text) = std::str::from_utf8(&buf[..read]) {
                if let Some(timestamp) = text.split("::").next() {
                    if !timestamp.is_empty() {
                        return timestamp.to_string();
                    }
                }
            }
        }
    }
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn merges_complete_files_and_mirrors_active() {
        let dir =
            std::env::temp_dir().join(format!("cla-consolidation-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Two complete (older) files and one active (newest) one. Give the
        // active file the latest mtime by writing it last.
        write(
            &dir,
            "combatlog_2026-07-19_10-00-00.log",
            "26:07:19:10:00:00.0::a\n",
        );
        write(
            &dir,
            "combatlog_2026-07-19_11-00-00.log",
            "26:07:19:11:00:00.0::b\n",
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
        write(
            &dir,
            "combatlog_2026-07-19_12-00-00.log",
            "26:07:19:12:00:00.0::c\n",
        );

        let mut consolidator = Consolidator::new(&dir);
        consolidator.tick(None);

        // Complete files are merged and deleted; the active one is kept.
        assert!(!dir.join("combatlog_2026-07-19_10-00-00.log").exists());
        assert!(!dir.join("combatlog_2026-07-19_11-00-00.log").exists());
        assert!(dir.join("combatlog_2026-07-19_12-00-00.log").exists());

        let merged = std::fs::read_to_string(&consolidator.target).unwrap();
        assert_eq!(
            merged,
            "26:07:19:10:00:00.0::a\n26:07:19:11:00:00.0::b\n26:07:19:12:00:00.0::c\n"
        );

        // Active file grows; a second tick appends only the new bytes (no dup).
        std::fs::OpenOptions::new()
            .append(true)
            .open(dir.join("combatlog_2026-07-19_12-00-00.log"))
            .unwrap()
            .write_all(b"26:07:19:12:00:01.0::d\n")
            .unwrap();
        consolidator.tick(None);
        let merged = std::fs::read_to_string(&consolidator.target).unwrap();
        assert_eq!(
            merged.matches("::c").count(),
            1,
            "active must not be duplicated"
        );
        assert!(merged.ends_with("26:07:19:12:00:01.0::d\n"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Safety check against a COPY of the real rotating logs: every line must
    /// survive consolidation exactly once (no loss, no duplication). Point
    /// `CLA_TEST_LOG_DIR` at the game's GameClient log folder and run with:
    /// `CLA_TEST_LOG_DIR=<path> cargo test verify_against_real_logs -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads the real STO log folder"]
    fn verify_against_real_logs() {
        let Some(real) = std::env::var_os("CLA_TEST_LOG_DIR") else {
            println!("set CLA_TEST_LOG_DIR to the GameClient log folder to run this test");
            return;
        };
        let real = Path::new(&real);
        let dir = std::env::temp_dir().join("cla-consolidation-real");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut total_lines = 0usize;
        for entry in std::fs::read_dir(real).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("combatlog_") && name.ends_with(".log") {
                let content = std::fs::read(entry.path()).unwrap();
                total_lines += content.iter().filter(|&&b| b == b'\n').count();
                std::fs::write(dir.join(name), content).unwrap();
            }
        }

        let mut consolidator = Consolidator::new(&dir);
        consolidator.tick(None);

        let merged = std::fs::read(&consolidator.target).unwrap();
        let merged_lines = merged.iter().filter(|&&b| b == b'\n').count();
        println!("source lines: {total_lines}, merged lines: {merged_lines}");
        assert_eq!(
            merged_lines, total_lines,
            "line count changed after consolidation"
        );

        // Only the active file should remain alongside the consolidated target.
        let remaining: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("combatlog_") && n.ends_with(".log"))
            .collect();
        println!("remaining rotated files: {remaining:?}");
        assert_eq!(remaining.len(), 1, "exactly the active file should remain");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_range_detects_mismatch_and_accepts_match() {
        let dir =
            std::env::temp_dir().join(format!("cla-consolidation-verify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = dir.join("src");
        std::fs::write(&src, b"hello world").unwrap();

        // A merged file whose tail matches the source: verification passes.
        let good = dir.join("good");
        std::fs::write(&good, b"XXXXhello world").unwrap();
        assert!(verify_range(&src, 0, &good, 4, 11).is_ok());

        // A merged file whose tail differs: verification must reject it, so the
        // caller never deletes the original.
        let bad = dir.join("bad");
        std::fs::write(&bad, b"XXXXhello WORLD").unwrap();
        assert!(verify_range(&src, 0, &bad, 4, 11).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preserves_every_byte_including_large_files() {
        let dir =
            std::env::temp_dir().join(format!("cla-consolidation-large-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // One complete file larger than the verification chunk, to exercise the
        // multi-chunk read-back path, plus a small one; newest is active.
        let big: String = (0..5000)
            .map(|i| format!("26:07:19:10:{:02}:{:02}.0::line-{i}\n", i / 60 % 60, i % 60))
            .collect();
        assert!(
            big.len() > VERIFY_CHUNK,
            "big file must span multiple chunks"
        );
        write(&dir, "combatlog_2026-07-19_10-00-00.log", &big);
        write(
            &dir,
            "combatlog_2026-07-19_11-00-00.log",
            "26:07:19:11:00:00.0::mid\n",
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
        write(
            &dir,
            "combatlog_2026-07-19_12-00-00.log",
            "26:07:19:12:00:00.0::active\n",
        );

        let expected = format!("{big}26:07:19:11:00:00.0::mid\n26:07:19:12:00:00.0::active\n");

        let mut consolidator = Consolidator::new(&dir);
        consolidator.tick(None);

        let merged = std::fs::read_to_string(&consolidator.target).unwrap();
        assert_eq!(merged, expected, "every byte must survive exactly once");
        // Only the active (newest) rotated file remains.
        assert!(dir.join("combatlog_2026-07-19_12-00-00.log").exists());
        assert!(!dir.join("combatlog_2026-07-19_10-00-00.log").exists());
        assert!(!dir.join("combatlog_2026-07-19_11-00-00.log").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restart_does_not_reduplicate_active() {
        let dir =
            std::env::temp_dir().join(format!("cla-consolidation-restart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write(
            &dir,
            "combatlog_2026-07-19_12-00-00.log",
            "26:07:19:12:00:00.0::c\n",
        );

        let mut consolidator = Consolidator::new(&dir);
        consolidator.tick(None);
        // Simulate a restart: a fresh Consolidator loads the persisted offset.
        let mut restarted = Consolidator::new(&dir);
        restarted.tick(None);

        let merged = std::fs::read_to_string(&restarted.target).unwrap();
        assert_eq!(
            merged.matches("::c").count(),
            1,
            "restart re-appended active"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
