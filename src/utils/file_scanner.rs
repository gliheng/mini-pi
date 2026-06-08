use std::path::{Path, PathBuf};

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    ".next",
    ".nuxt",
    "__pycache__",
    ".venv",
    "venv",
    ".tox",
    "build",
    ".cache",
    "vendor",
    ".cargo",
    ".rustup",
    ".npm",
    ".yarn",
    ".pnpm-store",
    "bower_components",
];

const MAX_ENTRIES: usize = 500;
const MAX_DEPTH: u32 = 4;

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub relative_path: String,
    pub is_dir: bool,
}

fn should_skip(name: &str) -> bool {
    name.starts_with('.') || SKIP_DIRS.contains(&name)
}

fn scan_recursive(
    dir: &Path,
    root: &Path,
    depth: u32,
    entries: &mut Vec<FileEntry>,
) {
    if depth > MAX_DEPTH || entries.len() >= MAX_ENTRIES {
        return;
    }

    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };

    let mut sub_dirs: Vec<PathBuf> = Vec::new();
    let mut file_entries: Vec<FileEntry> = Vec::new();

    for entry in read_dir.flatten() {
        if entries.len() >= MAX_ENTRIES {
            break;
        }

        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        if should_skip(&name) {
            continue;
        }

        let is_dir = path.is_dir();
        let relative_path = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        let entry = FileEntry {
            name,
            path: path.clone(),
            relative_path,
            is_dir,
        };

        if is_dir {
            sub_dirs.push(path);
            entries.push(entry);
        } else {
            file_entries.push(entry);
        }
    }

    entries.extend(file_entries);

    for sub_dir in sub_dirs {
        if entries.len() < MAX_ENTRIES {
            scan_recursive(&sub_dir, root, depth + 1, entries);
        }
    }
}

pub fn scan_directory(root: &Path) -> Vec<FileEntry> {
    if !root.is_dir() {
        return Vec::new();
    }
    let mut entries = Vec::new();
    scan_recursive(root, root, 0, &mut entries);
    entries.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.relative_path.cmp(&b.relative_path),
        }
    });
    entries
}

pub fn filter_entries<'a>(entries: &'a [FileEntry], query: &str) -> Vec<&'a FileEntry> {
    if query.is_empty() {
        return entries.iter().take(50).collect();
    }
    let q = query.to_lowercase();
    entries
        .iter()
        .filter(|e| {
            e.name.to_lowercase().contains(&q)
                || e.relative_path.to_lowercase().contains(&q)
        })
        .take(50)
        .collect()
}