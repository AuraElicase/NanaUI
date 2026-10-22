//! Fixture discovery shared by the integration tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use nana_ui_diagram::DiagramKind;

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

/// Every fixture:
/// `(family, name, source)`.
pub fn fixtures() -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for kind in DiagramKind::ALL {
        let dir = root().join("fixtures").join(kind.name());
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "mmd"))
            .collect();
        files.sort();
        for file in files {
            let name = file.file_stem().unwrap().to_string_lossy().into_owned();
            let source = std::fs::read_to_string(&file).unwrap();
            out.push((kind.name().to_string(), name, source));
        }
    }
    out
}
