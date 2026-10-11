//! Bounded local archives. No extraction to paths supplied by an archive.
use crate::catalog::relative_path;
use crate::paths::Paths;
use crate::{Error, Result};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

pub const MAX_ARCHIVE: u64 = 32 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 128;
pub fn name(kind: &str) -> String {
    let date = arcade_link::manifest::now_rfc3339().replace(':', "-");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("arcade-{kind}-{date}-{nonce}.zip")
}

pub fn write(paths: &Paths, name: &str, files: &BTreeMap<String, Vec<u8>>) -> Result<PathBuf> {
    relative_path(name)?;
    let directory = paths.state().join("exports");
    paths.guard(&directory)?;
    arcade_link::paths::ensure_private_dir(&directory)?;
    let mut staged = tempfile::NamedTempFile::new_in(&directory)?;
    let mut zip = zip::ZipWriter::new(staged.as_file_mut());
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o600);
    let mut size = 0u64;
    if files.len() > MAX_ENTRIES {
        return Err(Error::new("too_large", "Archive has too many files."));
    }
    for (name, bytes) in files {
        relative_path(name)?;
        size = size.saturating_add(bytes.len() as u64);
        if size > MAX_ARCHIVE {
            return Err(Error::new("too_large", "Archive exceeds 32 MiB."));
        }
        zip.start_file(name, options).map_err(zip_error)?;
        zip.write_all(bytes)?;
    }
    zip.finish().map_err(zip_error)?;
    staged.as_file().sync_all()?;
    let destination = directory.join(name);
    if destination.exists() {
        return Err(Error::new("exists", "This archive already exists."));
    }
    staged
        .persist_noclobber(&destination)
        .map_err(|e| Error::from(e.error))?;
    Ok(destination)
}

pub fn read(paths: &Paths, file: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    paths.guard(file)?;
    if fs::metadata(file)?.len() > MAX_ARCHIVE {
        return Err(Error::new("too_large", "Archive exceeds 32 MiB."));
    }
    let mut zip = zip::ZipArchive::new(fs::File::open(file)?).map_err(zip_error)?;
    if zip.len() > MAX_ENTRIES {
        return Err(Error::new("too_large", "Archive has too many files."));
    }
    let mut files = BTreeMap::new();
    let mut remaining = MAX_ARCHIVE;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(zip_error)?;
        relative_path(entry.name())?;
        if entry.is_dir()
            || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
            || entry.size() > remaining
            || files.contains_key(entry.name())
        {
            return Err(Error::new(
                "archive",
                "Unsafe, duplicate or oversized archive entry.",
            ));
        }
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        (&mut entry).take(remaining + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > remaining {
            return Err(Error::new("too_large", "Expanded archive exceeds 32 MiB."));
        }
        remaining -= bytes.len() as u64;
        files.insert(name, bytes);
    }
    Ok(files)
}

fn zip_error(error: zip::result::ZipError) -> Error {
    Error::new("archive", error.to_string())
}
