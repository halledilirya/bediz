#![cfg(windows)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use bediz_core::delivery::SaveError;
use bediz_core::file_name::{self, LocalTime};
use bediz_platform::save_screenshot;

const TIME: LocalTime = LocalTime {
    year: 2026,
    month: 11,
    day: 5,
    hour: 14,
    minute: 30,
    second: 12,
};

fn empty_folder(name: &str) -> io::Result<PathBuf> {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("save")
        .join(name);
    if folder.exists() {
        fs::remove_dir_all(&folder)?;
    }
    fs::create_dir_all(&folder)?;
    Ok(folder)
}

fn names(folder: &Path) -> io::Result<Vec<String>> {
    let mut names = fs::read_dir(folder)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<io::Result<Vec<_>>>()?;
    names.sort();
    Ok(names)
}

#[test]
fn saving_twice_in_one_second_keeps_both_files() {
    let folder = empty_folder("twice").unwrap();
    let first = save_screenshot(&folder, &TIME, b"first").unwrap();
    let second = save_screenshot(&folder, &TIME, b"second").unwrap();
    assert_eq!(first, folder.join(file_name::screenshot(&TIME, 1)));
    assert_eq!(second, folder.join(file_name::screenshot(&TIME, 2)));
    assert_eq!(fs::read(&first).unwrap(), b"first");
    assert_eq!(fs::read(&second).unwrap(), b"second");
    assert_eq!(names(&folder).unwrap().len(), 2);
}

#[test]
fn an_existing_file_is_never_replaced() {
    let folder = empty_folder("existing").unwrap();
    let taken = folder.join(file_name::screenshot(&TIME, 1));
    fs::write(&taken, b"mine").unwrap();
    let saved = save_screenshot(&folder, &TIME, b"new").unwrap();
    assert_eq!(fs::read(&taken).unwrap(), b"mine");
    assert_eq!(saved, folder.join(file_name::screenshot(&TIME, 2)));
    assert_eq!(fs::read(&saved).unwrap(), b"new");
}

#[test]
fn a_leftover_temporary_file_is_left_alone() {
    let folder = empty_folder("leftover").unwrap();
    let name = file_name::screenshot(&TIME, 1);
    let leftover = folder.join(file_name::temporary(&name));
    fs::write(&leftover, b"crashed").unwrap();
    let saved = save_screenshot(&folder, &TIME, b"new").unwrap();
    assert_eq!(saved, folder.join(&name));
    assert_eq!(fs::read(&leftover).unwrap(), b"crashed");
    assert_eq!(
        names(&folder).unwrap(),
        [name.clone(), file_name::temporary(&name)]
    );
}

#[test]
fn a_missing_folder_is_reported() {
    let folder = empty_folder("missing").unwrap().join("gone");
    assert_eq!(
        save_screenshot(&folder, &TIME, b"new"),
        Err(SaveError::FolderMissing)
    );
}
