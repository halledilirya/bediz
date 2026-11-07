#![cfg(windows)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use bediz_core::delivery::SaveError;
use bediz_core::file_name;
use bediz_platform::{
    SavedAs, file_id, keep_beside, replace_own_file, save_beside, save_chosen, save_folder,
    save_screenshot,
};

const STEM: &str = "Bediz 2026-11-05 143012";

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
    let first = save_screenshot(&folder, STEM, b"first").unwrap();
    let second = save_screenshot(&folder, STEM, b"second").unwrap();
    assert_eq!(first, folder.join(file_name::png(STEM, 1)));
    assert_eq!(second, folder.join(file_name::png(STEM, 2)));
    assert_eq!(fs::read(&first).unwrap(), b"first");
    assert_eq!(fs::read(&second).unwrap(), b"second");
    assert_eq!(names(&folder).unwrap().len(), 2);
}

#[test]
fn a_name_without_the_time_keeps_counting() {
    let folder = empty_folder("counting").unwrap();
    for _ in 0..12 {
        save_screenshot(&folder, "Notes", b"png").unwrap();
    }
    let saved = names(&folder).unwrap();
    assert_eq!(saved.len(), 12);
    assert!(saved.contains(&"Notes.png".to_owned()));
    assert!(saved.contains(&"Notes (12).png".to_owned()));
}

#[test]
fn an_existing_file_is_never_replaced() {
    let folder = empty_folder("existing").unwrap();
    let taken = folder.join(file_name::png(STEM, 1));
    fs::write(&taken, b"mine").unwrap();
    let saved = save_screenshot(&folder, STEM, b"new").unwrap();
    assert_eq!(fs::read(&taken).unwrap(), b"mine");
    assert_eq!(saved, folder.join(file_name::png(STEM, 2)));
    assert_eq!(fs::read(&saved).unwrap(), b"new");
}

#[test]
fn a_leftover_temporary_file_is_left_alone() {
    let folder = empty_folder("leftover").unwrap();
    let name = file_name::png(STEM, 1);
    let leftover = folder.join(file_name::temporary(&name));
    fs::write(&leftover, b"crashed").unwrap();
    let saved = save_screenshot(&folder, STEM, b"new").unwrap();
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
    assert_eq!(saved, SavedAs::Saved(chosen.clone()));
    assert_eq!(fs::read(&chosen).unwrap(), b"new");
    assert_eq!(fs::read(&leftover).unwrap(), b"crashed");
    assert_eq!(names(&folder).unwrap().len(), 2);
}

#[test]
fn a_file_that_appeared_after_the_choice_is_kept() {
    let folder = empty_folder("appeared").unwrap();
    let chosen = folder.join("notes.png");
    fs::write(&chosen, b"other").unwrap();
    assert_eq!(save_chosen(&chosen, b"new", None), Ok(SavedAs::NameTaken));
    assert_eq!(fs::read(&chosen).unwrap(), b"other");
    assert_eq!(names(&folder).unwrap(), ["notes.png".to_owned()]);
}

#[test]
fn a_save_whose_name_was_taken_takes_the_next_free_one_beside_it() {
    let folder = empty_folder("beside").unwrap();
    let chosen = folder.join("notes.png");
    fs::write(&chosen, b"other").unwrap();
    let saved = save_beside(&chosen, b"new").unwrap();
    assert_eq!(saved, folder.join("notes (2).png"));
    assert_eq!(fs::read(&chosen).unwrap(), b"other");
    assert_eq!(fs::read(&saved).unwrap(), b"new");
}

#[test]
fn a_taken_name_at_the_length_limit_still_finds_a_name_beside_it() {
    let folder = empty_folder("beside-long").unwrap();
    let chosen = folder.join(format!("{}.png", "a".repeat(248)));
    fs::write(&chosen, b"other").unwrap();
    let saved = save_beside(&chosen, b"new").unwrap();
    assert_eq!(saved, folder.join(format!("{} (2).png", "a".repeat(247))));
    assert_eq!(fs::read(&chosen).unwrap(), b"other");
    assert_eq!(fs::read(&saved).unwrap(), b"new");
    assert_eq!(names(&folder).unwrap().len(), 2);
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
        Ok(SavedAs::NameTaken)
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
    assert_eq!(
        save_chosen(&chosen, b"new", Some(asked)),
        Ok(SavedAs::Saved(chosen.clone()))
    );
    assert_eq!(fs::read(&chosen).unwrap(), b"new");
}

#[test]
fn a_new_chosen_name_is_saved() {
    let folder = empty_folder("new-name").unwrap();
    let chosen = folder.join("notes.png");
    assert_eq!(
        save_chosen(&chosen, b"new", None),
        Ok(SavedAs::Saved(chosen.clone()))
    );
    assert_eq!(fs::read(&chosen).unwrap(), b"new");
}

#[test]
fn a_name_at_the_length_limit_is_saved() {
    let folder = empty_folder("long-name").unwrap();
    let chosen = folder.join(format!("{}.png", "a".repeat(248)));
    assert_eq!(
        save_chosen(&chosen, b"new", None),
        Ok(SavedAs::Saved(chosen.clone()))
    );
    assert_eq!(fs::read(&chosen).unwrap(), b"new");
}

#[test]
fn the_apps_own_file_is_replaced_and_no_temporary_is_left() {
    let folder = empty_folder("own-file").unwrap();
    let own = folder.join("settings.json");
    replace_own_file(&own, b"first").unwrap();
    replace_own_file(&own, b"second").unwrap();
    assert_eq!(fs::read(&own).unwrap(), b"second");
    assert_eq!(names(&folder).unwrap(), ["settings.json".to_owned()]);
}

#[test]
fn a_link_left_at_a_temporary_name_never_leads_the_write_into_another_file() {
    let folder = empty_folder("own-file-link").unwrap();
    let elsewhere = empty_folder("own-file-link-target")
        .unwrap()
        .join("sentinel");
    fs::write(&elsewhere, b"untouched").unwrap();
    let own = folder.join("settings.json");
    for attempt in 1..=3 {
        let name = file_name::temporary(&file_name::numbered("settings.json", attempt));
        fs::hard_link(&elsewhere, folder.join(name)).unwrap();
    }
    replace_own_file(&own, b"settings").unwrap();
    assert_eq!(fs::read(&own).unwrap(), b"settings");
    assert_eq!(fs::read(&elsewhere).unwrap(), b"untouched");
}

#[test]
fn a_file_kept_beside_keeps_its_content_when_the_file_is_replaced() {
    let folder = empty_folder("kept").unwrap();
    let own = folder.join("settings.json");
    fs::write(&own, b"unreadable").unwrap();
    let kept = keep_beside(&own, "settings.unreadable.json", 100).unwrap();
    assert_eq!(kept, folder.join("settings.unreadable.json"));
    assert_eq!(
        keep_beside(&own, "settings.unreadable.json", 100).unwrap(),
        kept
    );
    replace_own_file(&own, b"readable").unwrap();
    assert_eq!(fs::read(&kept).unwrap(), b"unreadable");
    assert_eq!(fs::read(&own).unwrap(), b"readable");
    replace_own_file(&own, b"unreadable again").unwrap();
    let again = keep_beside(&own, "settings.unreadable.json", 100).unwrap();
    assert_eq!(again, folder.join("settings.unreadable (2).json"));
    assert_eq!(fs::read(&kept).unwrap(), b"unreadable");
    assert_eq!(fs::read(&again).unwrap(), b"unreadable again");
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
        save_screenshot(&folder, STEM, b"new"),
        Err(SaveError::FolderMissing)
    );
}

#[test]
fn a_chosen_folder_is_used_as_it_is_and_never_a_relative_one() {
    let folder = empty_folder("chosen-folder").unwrap();
    assert_eq!(save_folder(Some(&folder)), Ok(folder));
    assert_eq!(
        save_folder(Some(Path::new("Pictures"))),
        Err(SaveError::FolderMissing)
    );
}
