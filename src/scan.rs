use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use rayon::prelude::*;

/// Shared counters the UI polls while a scan is running.
#[derive(Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub cancel: AtomicBool,
    pub current: Mutex<String>,
}

/// A directory in the scanned tree. Nodes live in a flat arena and refer to each other by index.
pub struct Dir {
    pub name: String,
    pub parent: Option<usize>,
    /// Child directories, sorted largest first.
    pub children: Vec<usize>,
    /// Total size of everything below this directory.
    pub size: u64,
    /// Size of the files directly inside this directory.
    pub files_size: u64,
    pub file_count: u64,
}

pub struct Tree {
    pub root_path: PathBuf,
    pub dirs: Vec<Dir>,
}

impl Tree {
    pub const ROOT: usize = 0;

    pub fn path(&self, id: usize) -> PathBuf {
        let mut parts = Vec::new();
        let mut cur = id;
        while let Some(parent) = self.dirs[cur].parent {
            parts.push(self.dirs[cur].name.as_str());
            cur = parent;
        }
        let mut path = self.root_path.clone();
        path.extend(parts.iter().rev());
        path
    }

    /// Chain of ancestors from the root down to `id` (inclusive).
    pub fn ancestry(&self, id: usize) -> Vec<usize> {
        let mut chain = vec![id];
        let mut cur = id;
        while let Some(parent) = self.dirs[cur].parent {
            chain.push(parent);
            cur = parent;
        }
        chain.reverse();
        chain
    }
}

struct RawDir {
    name: String,
    size: u64,
    files_size: u64,
    file_count: u64,
    children: Vec<RawDir>,
}

pub fn scan(root: &Path, progress: &Progress) -> Tree {
    let device = fs::symlink_metadata(root).map(|m| device_of(&m)).unwrap_or(0);
    let name = root.to_string_lossy().into_owned();
    let raw = scan_dir(root, name, device, progress);

    let mut dirs = Vec::with_capacity(progress.dirs.load(Ordering::Relaxed) as usize + 1);
    flatten(raw, None, &mut dirs);
    Tree { root_path: root.to_path_buf(), dirs }
}

fn scan_dir(path: &Path, name: String, device: u64, progress: &Progress) -> RawDir {
    let mut dir = RawDir { name, size: 0, files_size: 0, file_count: 0, children: Vec::new() };
    if progress.cancel.load(Ordering::Relaxed) {
        return dir;
    }
    progress.dirs.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut current) = progress.current.try_lock() {
        current.clear();
        current.push_str(&path.to_string_lossy());
    }

    let Ok(entries) = fs::read_dir(path) else {
        return dir;
    };

    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        // DirEntry::metadata does not follow symlinks, so links are counted as tiny files.
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            // Stay on one filesystem, so mounted volumes aren't counted.
            if device_of(&meta) == device && !is_firmlink_mirror(&entry.path()) {
                subdirs.push(entry);
            }
        } else {
            dir.files_size += disk_size(&meta);
            dir.file_count += 1;
        }
    }
    progress.files.fetch_add(dir.file_count, Ordering::Relaxed);
    progress.bytes.fetch_add(dir.files_size, Ordering::Relaxed);

    dir.children = subdirs
        .into_par_iter()
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            scan_dir(&entry.path(), name, device, progress)
        })
        .collect();
    dir.children.sort_unstable_by_key(|c| std::cmp::Reverse(c.size));
    dir.size = dir.files_size + dir.children.iter().map(|c| c.size).sum::<u64>();
    dir
}

fn flatten(raw: RawDir, parent: Option<usize>, dirs: &mut Vec<Dir>) -> usize {
    let id = dirs.len();
    dirs.push(Dir {
        name: raw.name,
        parent,
        children: Vec::with_capacity(raw.children.len()),
        size: raw.size,
        files_size: raw.files_size,
        file_count: raw.file_count,
    });
    for child in raw.children {
        let child_id = flatten(child, Some(id), dirs);
        dirs[id].children.push(child_id);
    }
    id
}

/// On macOS the Data volume shares a device id with `/` and its contents are already reachable
/// through firmlinks like `/Users`, so scanning it too would count everything twice.
fn is_firmlink_mirror(path: &Path) -> bool {
    cfg!(target_os = "macos") && path == Path::new("/System/Volumes/Data")
}

#[cfg(unix)]
fn device_of(meta: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.dev()
}

#[cfg(not(unix))]
fn device_of(_meta: &Metadata) -> u64 {
    0
}

/// Space actually allocated on disk (sparse and compressed files report less than their length).
#[cfg(unix)]
fn disk_size(meta: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks() * 512
}

#[cfg(not(unix))]
fn disk_size(meta: &Metadata) -> u64 {
    meta.len()
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}
