//! Delta downloads (DECISIONS.md D5): which local files can supply chunks of a new installer, and how the missing
//! chunks become a few bounded HTTP Range requests. Pure planning; `source.rs` does the I/O.
use crate::installation;
use crate::paths::Paths;
use arcade_link::release::{plan_delta, ChunkIndex, DeltaPlan, FileEntry, Piece};
use std::ops::Range;
use std::path::{Path, PathBuf};

/// One assembled `Fetch` piece is at most this long, so a piece fits in memory.
pub const PIECE_MAX: u64 = 4 * 1024 * 1024;
/// Missing ranges closer than this share one request: re-downloading a small gap is cheaper than another round trip.
pub const GAP_MAX: u64 = 256 * 1024;
/// One Range request covers at most this many bytes (one buffer).
pub const REQUEST_MAX: u64 = 8 * 1024 * 1024;
/// A delta is worth it only when it skips at least a fifth of the file.
const WORTH: f64 = 0.8;
/// Files larger than this are not chunked on demand (it reads the whole file).
const CHUNK_ON_DEMAND_MAX: u64 = 512 * 1024 * 1024;

/// Where the saved manifest entry of a retained file with this SHA-256 lives.
pub fn saved_entry(paths: &Paths, id: &str, sha256: &str) -> PathBuf {
    installation::installers(paths, id).join(format!("{sha256}.manifest.json"))
}

pub fn save_entry(paths: &Paths, id: &str, entry: &FileEntry) -> crate::Result<()> {
    let path = saved_entry(paths, id, &entry.sha256);
    paths.guard(&path)?;
    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir)?;
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut file, &serde_json::to_vec(entry)?)?;
    file.persist(&path).map_err(|e| e.error)?;
    Ok(())
}

fn load_entry(paths: &Paths, id: &str, sha256: &str) -> Option<FileEntry> {
    let entry: FileEntry =
        serde_json::from_slice(&std::fs::read(saved_entry(paths, id, sha256)).ok()?).ok()?;
    (entry.sha256 == sha256).then_some(entry)
}

/// Indexes the app's retained installers and its installed file. `installed` is the installed executable and its
/// recorded SHA-256. A file whose saved entry exists is indexed without reading it; the installed file is chunked
/// once on demand and its entry saved for next time. Worker thread.
pub fn local_chunks(paths: &Paths, id: &str, installed: Option<(&Path, &str)>) -> ChunkIndex {
    let mut index = ChunkIndex::default();
    let mut seen = Vec::new();
    if let Ok(entries) = std::fs::read_dir(installation::installers(paths, id)) {
        for file in entries.flatten().map(|e| e.path()) {
            let Some(sha) = file
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.split_once('.'))
                .filter(|(sha, ext)| sha.len() == 64 && *ext != "manifest.json")
                .map(|(sha, _)| sha.to_owned())
            else {
                continue;
            };
            if let Some(entry) = load_entry(paths, id, &sha) {
                if std::fs::metadata(&file).is_ok_and(|m| m.len() == entry.size) {
                    index.add_known(&file, &entry);
                    seen.push(sha);
                }
            }
        }
    }
    if let Some((path, sha)) =
        installed.filter(|(_, sha)| sha.len() == 64 && !seen.iter().any(|s| s == sha))
    {
        match load_entry(paths, id, sha) {
            Some(entry) if std::fs::metadata(path).is_ok_and(|m| m.len() == entry.size) => {
                index.add_known(path, &entry)
            }
            _ if std::fs::metadata(path).is_ok_and(|m| m.len() <= CHUNK_ON_DEMAND_MAX) => {
                if let Ok(entry) = FileEntry::from_path(path) {
                    if entry.sha256 == sha {
                        let _ = save_entry(paths, id, &entry);
                        index.add_known(path, &entry);
                    }
                }
            }
            _ => {}
        }
    }
    index
}

/// The delta for `entry`, or `None` when downloading the whole file is as good. `Fetch` pieces are split at chunk
/// boundaries to at most [`PIECE_MAX`] bytes.
pub fn plan(have: &ChunkIndex, entry: &FileEntry) -> Option<DeltaPlan> {
    if have.is_empty() {
        return None;
    }
    let plan = plan_delta(have, entry);
    if plan.download_bytes() as f64 > entry.size as f64 * WORTH {
        return None;
    }
    let boundaries: Vec<u64> = entry.ranges().map(|(r, _)| r.end).collect();
    let mut pieces = Vec::with_capacity(plan.pieces.len());
    for piece in plan.pieces {
        let Piece::Fetch { range } = piece else {
            pieces.push(piece);
            continue;
        };
        // Cut at the last chunk boundary that keeps the piece within PIECE_MAX (a chunk is at most 32 KiB).
        let mut start = range.start;
        let mut last = range.start;
        for &boundary in boundaries
            .iter()
            .filter(|&&b| b > range.start && b <= range.end)
        {
            if boundary - start > PIECE_MAX && last > start {
                pieces.push(Piece::Fetch { range: start..last });
                start = last;
            }
            last = boundary;
        }
        pieces.push(Piece::Fetch {
            range: start..range.end,
        });
    }
    Some(DeltaPlan { pieces })
}

/// The HTTP requests for a plan's missing ranges: in order, neighbours less than [`GAP_MAX`] apart merged, each at
/// most [`REQUEST_MAX`] long.
pub fn requests(plan: &DeltaPlan) -> Vec<Range<u64>> {
    let mut out: Vec<Range<u64>> = Vec::new();
    for range in plan.missing_ranges() {
        match out.last_mut() {
            Some(last)
                if range.start - last.end < GAP_MAX && range.end - last.start <= REQUEST_MAX =>
            {
                last.end = range.end
            }
            _ => out.push(range),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic, incompressible-looking bytes.
    fn noise(len: usize, seed: u64) -> Vec<u8> {
        let mut x = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect()
    }

    fn index_of(dir: &Path, data: &[u8]) -> ChunkIndex {
        let path = dir.join("old");
        std::fs::write(&path, data).unwrap();
        let mut index = ChunkIndex::default();
        index.add_file(&path).unwrap();
        index
    }

    #[test]
    fn pieces_stay_bounded_and_cover_the_missing_bytes_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let old = noise(12 * 1024 * 1024, 1);
        let mut new = old.clone();
        // A 9 MiB rewrite in the middle and a small edit near the end.
        new[1024 * 1024..10 * 1024 * 1024].copy_from_slice(&noise(9 * 1024 * 1024, 2));
        new[11 * 1024 * 1024 + 5] ^= 0xff;
        let entry = FileEntry::from_bytes("new".into(), &new);
        let index = index_of(dir.path(), &old);
        let plan = plan(&index, &entry).expect("a delta saves most of the file");
        let raw = plan_delta(&index, &entry);
        assert_eq!(plan.download_bytes(), raw.download_bytes());
        assert!(plan
            .missing_ranges()
            .iter()
            .all(|r| r.end - r.start <= PIECE_MAX + u64::from(arcade_link::release::CHUNK_MAX)));
        let requests = requests(&plan);
        assert!(requests.iter().all(|r| r.end - r.start <= REQUEST_MAX));
        assert!(requests.len() >= 2);
        // Assembling from the split plan produces the new file.
        let mut out = Vec::new();
        arcade_link::release::assemble(
            &entry,
            &plan,
            &index,
            &mut |r| Ok(new[r.start as usize..r.end as usize].to_vec()),
            &mut out,
        )
        .unwrap();
        assert_eq!(out, new);
    }

    #[test]
    fn nearby_ranges_share_a_request_and_distant_ones_do_not() {
        let fetch = |range: Range<u64>| Piece::Fetch { range };
        let have = Piece::Have {
            source: 0,
            offset: 0,
            len: 1,
        };
        let plan = DeltaPlan {
            pieces: vec![
                fetch(0..100),
                have.clone(),
                fetch(200..300),
                have,
                fetch(300 + GAP_MAX..400 + GAP_MAX),
            ],
        };
        assert_eq!(requests(&plan), vec![0..300, 300 + GAP_MAX..400 + GAP_MAX]);
    }

    #[test]
    fn a_mostly_new_file_or_no_local_files_means_a_full_download() {
        let dir = tempfile::tempdir().unwrap();
        let entry = FileEntry::from_bytes("new".into(), &noise(256 * 1024, 3));
        assert!(plan(&ChunkIndex::default(), &entry).is_none());
        assert!(plan(&index_of(dir.path(), &noise(256 * 1024, 4)), &entry).is_none());
    }
}
