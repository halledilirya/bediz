use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn every_source_file_starts_by_saying_what_it_is_for() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut folders = vec![root.join("app/src-tauri/src")];
    for member in fs::read_dir(root.join("crates")).unwrap() {
        let member = member.unwrap().path();
        if member.is_dir() {
            folders.push(member.join("src"));
        }
    }
    let mut files: Vec<PathBuf> = Vec::new();
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }

    let mut missing: Vec<&PathBuf> = Vec::new();
    let mut crowded: Vec<&PathBuf> = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).unwrap();
        if !text.starts_with("//! ") {
            missing.push(file);
        } else if text.lines().find(|line| !line.starts_with("//!")) != Some("") {
            crowded.push(file);
        }
    }

    assert!(files.iter().any(|file| file.ends_with("src/lib.rs")));
    assert!(
        missing.is_empty(),
        "files without a first //! line: {missing:#?}"
    );
    assert!(
        crowded.is_empty(),
        "files without a blank line after their //! lines: {crowded:#?}"
    );
}
