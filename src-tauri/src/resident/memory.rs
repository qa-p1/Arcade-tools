//! Reclaim allocator slack and cold GUI library pages after window destruction.

#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn release_unused_runtime_pages() {
    // SAFETY: malloc_trim is thread-safe and only releases free allocations.
    unsafe { libc::malloc_trim(0) };
    let Ok(source) = std::fs::read_to_string("/proc/self/maps") else {
        return;
    };
    for (start, length) in library_pages(&source) {
        // SAFETY: PAGEOUT is a reclaim hint, preserving bytes even if a page
        // becomes dirty or the mapping changes after reading /proc. Unlike
        // DONTNEED it never discards private changes. Unsupported kernels
        // simply ignore the failed advisory call.
        unsafe { libc::madvise(start as *mut libc::c_void, length, libc::MADV_PAGEOUT) };
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn release_unused_runtime_pages() {}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn library_pages(source: &str) -> Vec<(usize, usize)> {
    let executable = std::env::current_exe().ok();
    source
        .lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            let permissions = *fields.get(1)?;
            let path = *fields.get(5)?;
            if !permissions.starts_with('r')
                || permissions.contains('w')
                || !path.starts_with('/')
                || line.ends_with("(deleted)")
                || !(path.contains(".so")
                    || executable
                        .as_ref()
                        .is_some_and(|p| p == std::path::Path::new(path)))
            {
                return None;
            }
            let (start, end) = fields[0].split_once('-')?;
            let start = usize::from_str_radix(start, 16).ok()?;
            let end = usize::from_str_radix(end, 16).ok()?;
            let length = end.checked_sub(start).filter(|length| *length > 0)?;
            Some((start, length))
        })
        .collect()
}

#[cfg(all(test, target_os = "linux", target_env = "gnu"))]
mod tests {
    use super::*;

    #[test]
    fn reclaim_only_read_only_file_mappings() {
        let maps = "1000-2000 r-xp 0 00:01 1 /usr/lib/libclean.so\n2000-3000 rw-p 0 00:01 1 /usr/lib/libwritable.so\n3000-4000 r--p 0 00:00 0\n4000-5000 r--p 0 00:01 1 /usr/lib/libgone.so (deleted)\n";
        assert_eq!(library_pages(maps), [(0x1000, 0x1000)]);
    }
}
