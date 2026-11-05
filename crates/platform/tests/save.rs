#![cfg(windows)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use bediz_core::delivery::SaveError;
use bediz_core::file_name::{self, LocalTime};
use bediz_platform::{file_id, save_chosen, save_screenshot};

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
fn a_chosen_path_replaces_the_file_the_user_confirmed() {
    let folder = empty_folder("chosen").unwrap();
    let chosen = folder.join("notes.png");
    let leftover = folder.join(file_name::scratch(1));
    fs::write(&chosen, b"old").unwrap();
    fs::write(&leftover, b"crashed").unwrap();
    let asked = file_id(&chosen).unwrap();
    let saved = save_chosen(&chosen, b"new", Some(asked)).unwrap();
    assert_eq!(saved, chosen);
    assert_eq!(fs::read(&chosen).unwrap(), b"new");
    assert_eq!(fs::read(&leftover).unwrap(), b"crashed");
    assert_eq!(names(&folder).unwrap().len(), 2);
}

#[test]
fn a_file_that_appeared_after_the_choice_is_kept() {
    let folder = empty_folder("appeared").unwrap();
    let chosen = folder.join("notes.png");
    fs::write(&chosen, b"other").unwrap();
    assert_eq!(save_chosen(&chosen, b"new", None), Err(SaveError::Appeared));
    assert_eq!(fs::read(&chosen).unwrap(), b"other");
    assert_eq!(names(&folder).unwrap(), ["notes.png".to_owned()]);
}

#[test]
fn a_file_that_took_the_asked_files_place_is_kept() {
    let folder = empty_folder("took-place").unwrap();
    let chosen = folder.join("notes.png");
    fs::write(&chosen, b"asked").unwrap();
    let asked = file_id(&chosen).unwrap();
    fs::remove_file(&chosen).unwrap();
    fs::write(&chosen, b"other").unwrap();
    assert_ne!(file_id(&chosen).unwrap(), asked);
    assert_eq!(
        save_chosen(&chosen, b"new", Some(asked)),
        Err(SaveError::Appeared)
    );
    assert_eq!(fs::read(&chosen).unwrap(), b"other");
}

#[test]
fn a_file_asked_about_and_then_removed_leaves_a_new_save() {
    let folder = empty_folder("asked-removed").unwrap();
    let chosen = folder.join("notes.png");
    fs::write(&chosen, b"asked").unwrap();
    let asked = file_id(&chosen).unwrap();
    fs::remove_file(&chosen).unwrap();
    assert_eq!(save_chosen(&chosen, b"new", Some(asked)).unwrap(), chosen);
    assert_eq!(fs::read(&chosen).unwrap(), b"new");
}

#[test]
fn a_new_chosen_name_is_saved() {
    let folder = empty_folder("new-name").unwrap();
    let chosen = folder.join("notes.png");
    assert_eq!(save_chosen(&chosen, b"new", None).unwrap(), chosen);
    assert_eq!(fs::read(&chosen).unwrap(), b"new");
}

#[test]
fn a_name_at_the_length_limit_is_saved() {
    let folder = empty_folder("long-name").unwrap();
    let chosen = folder.join(format!("{}.png", "a".repeat(248)));
    assert_eq!(save_chosen(&chosen, b"new", None).unwrap(), chosen);
    assert_eq!(fs::read(&chosen).unwrap(), b"new");
}

#[test]
fn a_chosen_path_in_a_missing_folder_is_reported() {
    let chosen = empty_folder("chosen-missing")
        .unwrap()
        .join("gone")
        .join("notes.png");
    assert_eq!(
        save_chosen(&chosen, b"new", None),
        Err(SaveError::FolderMissing)
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
