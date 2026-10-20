//! Small helpers every packager step shares.

use std::path::{Path, PathBuf};

/// Every file under `root`, sorted. Directories are descended without
/// following symlinks; a symlink to a file counts as that file.
pub(crate) fn walk_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let read = std::fs::read_dir(&dir)
            .map_err(|error| format!("cannot list {}: {error}", dir.display()))?;
        for item in read {
            let item = item.map_err(|error| error.to_string())?;
            let path = item.path();
            let kind = item.file_type().map_err(|error| error.to_string())?;
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() || path.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// `path` relative to `root`, `/`-separated.
pub(crate) fn relative_path(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| format!("{} is outside {}", path.display(), root.display()))?;
    relative
        .components()
        .map(|c| c.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.join("/"))
        .ok_or_else(|| format!("{} is not UTF-8", relative.display()))
}

/// Hex BLAKE3 of `bytes`, as manifests and Steam file lists record it.
pub(crate) fn content_hex(bytes: &[u8]) -> String {
    nana_package::to_hex(&nana_package::hash::content(bytes))
}

/// Copy a directory tree. Symlinks are recreated as symlinks (a `.framework`
/// links `Versions/Current`), not followed.
pub(crate) fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for item in std::fs::read_dir(from).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let target = to.join(item.file_name());
        let kind = item.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            let link = std::fs::read_link(item.path()).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(&link, &target).map_err(|e| e.to_string())?;
            #[cfg(windows)]
            {
                let resolved = item
                    .path()
                    .parent()
                    .map_or(link.clone(), |dir| dir.join(&link));
                if resolved.is_dir() {
                    std::os::windows::fs::symlink_dir(&link, &target)
                } else {
                    std::os::windows::fs::symlink_file(&link, &target)
                }
                .map_err(|e| e.to_string())?;
            }
        } else if kind.is_dir() {
            copy_tree(&item.path(), &target)?;
        } else {
            std::fs::copy(item.path(), &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Every symlink under `dir` that points outside it: absolute, or climbing
/// past `dir` with `..`. A bundle copied into a package must not reach out of
/// it.
pub(crate) fn escaping_links(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut escaping = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    while let Some((at, depth)) = stack.pop() {
        for item in std::fs::read_dir(&at).map_err(|e| e.to_string())? {
            let item = item.map_err(|e| e.to_string())?;
            let kind = item.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() {
                let link = std::fs::read_link(item.path()).map_err(|e| e.to_string())?;
                let mut level = depth as isize;
                let mut outside = link.has_root();
                for part in link.components() {
                    match part {
                        std::path::Component::ParentDir => level -= 1,
                        std::path::Component::Normal(_) => level += 1,
                        std::path::Component::CurDir => {}
                        _ => outside = true,
                    }
                    outside |= level < 0;
                }
                if outside {
                    escaping.push(item.path());
                }
            } else if kind.is_dir() {
                stack.push((item.path(), depth + 1));
            }
        }
    }
    escaping.sort();
    Ok(escaping)
}

pub(crate) fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

pub(crate) fn io(context: &'static str) -> impl Fn(std::io::Error) -> String {
    move |error| format!("{context}: {error}")
}
