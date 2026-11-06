use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use bediz_core::delivery::SaveError;
use bediz_core::file_name;
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_BAD_NETPATH, ERROR_DISK_FULL,
    ERROR_FILE_EXISTS, ERROR_HANDLE_DISK_FULL, ERROR_LOCK_VIOLATION, ERROR_PATH_NOT_FOUND,
    ERROR_SHARING_VIOLATION, HANDLE, WIN32_ERROR,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ID_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FileIdInfo, GetFileInformationByHandleEx, MOVE_FILE_FLAGS, MOVEFILE_REPLACE_EXISTING,
    MoveFileExW,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{FOLDERID_Screenshots, KF_FLAG_CREATE, SHGetKnownFolderPath};
use windows::core::{HRESULT, PCWSTR};

const LAST_ATTEMPT: u32 = 10_000;

pub fn folder(chosen: Option<&Path>) -> Result<PathBuf, SaveError> {
    match chosen {
        Some(folder) if folder.is_absolute() => Ok(folder.to_path_buf()),
        Some(_) => Err(SaveError::FolderMissing),
        None => screenshots_folder(),
    }
}

fn screenshots_folder() -> Result<PathBuf, SaveError> {
    let path = unsafe { SHGetKnownFolderPath(&FOLDERID_Screenshots, KF_FLAG_CREATE, None) }
        .map_err(|error| save_error(error.code(), error.message()))?;
    let folder = OsString::from_wide(unsafe { path.as_wide() });
    unsafe { CoTaskMemFree(Some(path.0.cast())) };
    Ok(PathBuf::from(folder))
}

pub fn save_screenshot(folder: &Path, stem: &str, png: &[u8]) -> Result<PathBuf, SaveError> {
    let temporary = write_temporary(
        folder,
        |attempt| file_name::temporary(&file_name::png(stem, attempt)),
        png,
    )?;
    for attempt in 1..=LAST_ATTEMPT {
        let target = folder.join(file_name::png(stem, attempt));
        match move_file(&temporary, &target, MOVE_FILE_FLAGS(0)) {
            Ok(()) => return Ok(target),
            Err(error) if is_taken(error.code()) => {}
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                return Err(save_error(error.code(), error.message()));
            }
        }
    }
    let _ = fs::remove_file(&temporary);
    Err(SaveError::NoFreeName)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileId {
    volume: u64,
    file: [u8; 16],
}

#[cfg(test)]
impl FileId {
    pub fn for_tests(number: u8) -> Self {
        Self {
            volume: 1,
            file: [number; 16],
        }
    }
}

pub fn file_id(path: &Path) -> io::Result<FileId> {
    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES.0)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
        .open(path)?;
    let mut info = FILE_ID_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    }
    .map_err(io::Error::from)?;
    Ok(FileId {
        volume: info.VolumeSerialNumber,
        file: info.FileId.Identifier,
    })
}

pub fn save_chosen(path: &Path, png: &[u8], replace: Option<FileId>) -> Result<PathBuf, SaveError> {
    let Some(folder) = path.parent() else {
        return Err(SaveError::FolderMissing);
    };
    let temporary = write_temporary(folder, file_name::scratch, png)?;
    // Only the very file the Save as dialog asked about may be replaced; any other one is kept.
    let flags = match replace {
        Some(asked) if file_id(path).ok() == Some(asked) => MOVEFILE_REPLACE_EXISTING,
        _ => MOVE_FILE_FLAGS(0),
    };
    match move_file(&temporary, path, flags) {
        Ok(()) => Ok(path.to_path_buf()),
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            if is_taken(error.code()) {
                Err(SaveError::Appeared)
            } else {
                Err(save_error(error.code(), error.message()))
            }
        }
    }
}

fn write_temporary(
    folder: &Path,
    name: impl Fn(u32) -> String,
    png: &[u8],
) -> Result<PathBuf, SaveError> {
    for attempt in 1..=LAST_ATTEMPT {
        let path = folder.join(name(attempt));
        let file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error(&error)),
        };
        return match fill(file, png) {
            Ok(()) => Ok(path),
            Err(error) => {
                let _ = fs::remove_file(&path);
                Err(io_error(&error))
            }
        };
    }
    Err(SaveError::NoFreeName)
}

fn fill(mut file: File, png: &[u8]) -> io::Result<()> {
    file.write_all(png)?;
    file.sync_all()
}

// std::fs::rename replaces an existing file; MoveFileExW without MOVEFILE_REPLACE_EXISTING never does.
fn move_file(from: &Path, to: &Path, flags: MOVE_FILE_FLAGS) -> windows::core::Result<()> {
    let (from, to) = (extended(from), extended(to));
    unsafe { MoveFileExW(PCWSTR(from.as_ptr()), PCWSTR(to.as_ptr()), flags) }
}

fn is_taken(code: HRESULT) -> bool {
    code == ERROR_ALREADY_EXISTS.to_hresult() || code == ERROR_FILE_EXISTS.to_hresult()
}

// Direct calls reach past MAX_PATH only with the \\?\ prefix, which std::fs adds by itself.
fn extended(path: &Path) -> Vec<u16> {
    let mut wide = extended_wide(&path.as_os_str().encode_wide().collect::<Vec<_>>());
    wide.push(0);
    wide
}

fn extended_wide(path: &[u16]) -> Vec<u16> {
    let wide = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
    if path.starts_with(&wide(r"\\?\")) {
        return path.to_vec();
    }
    match path.strip_prefix(wide(r"\\").as_slice()) {
        Some(share) => [wide(r"\\?\UNC\").as_slice(), share].concat(),
        None => [wide(r"\\?\").as_slice(), path].concat(),
    }
}

fn io_error(error: &io::Error) -> SaveError {
    match error.raw_os_error() {
        Some(code) => save_error(WIN32_ERROR(code as u32).to_hresult(), error.to_string()),
        None => SaveError::Other(error.to_string()),
    }
}

fn save_error(code: HRESULT, message: impl ToString) -> SaveError {
    let is = |error: WIN32_ERROR| code == error.to_hresult();
    if is(ERROR_DISK_FULL) || is(ERROR_HANDLE_DISK_FULL) {
        SaveError::DiskFull
    } else if is(ERROR_ACCESS_DENIED) {
        SaveError::AccessDenied
    } else if is(ERROR_PATH_NOT_FOUND) || is(ERROR_BAD_NETPATH) {
        SaveError::FolderMissing
    } else if is(ERROR_SHARING_VIOLATION) || is(ERROR_LOCK_VIOLATION) {
        SaveError::InUse
    } else {
        SaveError::Other(message.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn local_paths_get_the_verbatim_prefix() {
        assert_eq!(
            extended_wide(&wide(r"C:\Pictures\a.png")),
            wide(r"\\?\C:\Pictures\a.png")
        );
    }

    #[test]
    fn network_paths_get_the_unc_prefix() {
        assert_eq!(
            extended_wide(&wide(r"\\server\share\a.png")),
            wide(r"\\?\UNC\server\share\a.png")
        );
    }

    #[test]
    fn prefixed_paths_stay_as_they_are() {
        let path = wide(r"\\?\C:\Pictures\a.png");
        assert_eq!(extended_wide(&path), path);
    }

    #[test]
    fn system_errors_become_typed_save_errors() {
        let error = |code: WIN32_ERROR| save_error(code.to_hresult(), "other");
        assert_eq!(error(ERROR_DISK_FULL), SaveError::DiskFull);
        assert_eq!(error(ERROR_HANDLE_DISK_FULL), SaveError::DiskFull);
        assert_eq!(error(ERROR_ACCESS_DENIED), SaveError::AccessDenied);
        assert_eq!(error(ERROR_PATH_NOT_FOUND), SaveError::FolderMissing);
        assert_eq!(error(ERROR_SHARING_VIOLATION), SaveError::InUse);
        assert_eq!(
            error(ERROR_ALREADY_EXISTS),
            SaveError::Other("other".to_owned())
        );
    }

    #[test]
    fn taken_names_are_recognised() {
        assert!(is_taken(ERROR_ALREADY_EXISTS.to_hresult()));
        assert!(is_taken(ERROR_FILE_EXISTS.to_hresult()));
        assert!(!is_taken(ERROR_ACCESS_DENIED.to_hresult()));
    }
}
