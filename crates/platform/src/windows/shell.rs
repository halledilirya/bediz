use std::ffi::c_void;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::Foundation::{ERROR_FILENAME_EXCED_RANGE, HWND, MAX_PATH};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::Shell::{
    FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING,
    ILCreateFromPathW, ILFree, SHFILEOPSTRUCTW, SHFileOperationW, SHOpenFolderAndSelectItems,
};
use windows::core::{HSTRING, PCWSTR};

pub fn show_in_folder(path: &Path) -> io::Result<()> {
    let path = path.to_path_buf();
    in_apartment(move || select(&path))
}

pub fn move_to_recycle_bin(path: &Path, owner: isize) -> io::Result<bool> {
    let path = path.to_path_buf();
    in_apartment(move || recycle(&path, HWND(owner as *mut c_void)))
}

// The shell's file calls need a single-threaded apartment, so each gets a thread of its own.
fn in_apartment<T: Send + 'static>(
    work: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    std::thread::Builder::new()
        .name("bediz-shell".to_owned())
        .spawn(move || {
            let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
            let done = work();
            if com {
                unsafe { CoUninitialize() };
            }
            done
        })?
        .join()
        .unwrap_or_else(|_| Err(io::Error::other("the shell's thread stopped")))
}

// The shell's item lists and file operations take at most MAX_PATH characters, the null included.
fn fits_the_shell(path: &Path) -> io::Result<()> {
    if path.as_os_str().encode_wide().count() < MAX_PATH as usize {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(
            ERROR_FILENAME_EXCED_RANGE.0 as i32,
        ))
    }
}

fn select(path: &Path) -> io::Result<()> {
    fits_the_shell(path)?;
    let item = unsafe { ILCreateFromPathW(&HSTRING::from(path.as_os_str())) };
    if item.is_null() {
        return Err(io::ErrorKind::NotFound.into());
    }
    let shown = unsafe { SHOpenFolderAndSelectItems(item, None, 0) };
    unsafe { ILFree(Some(item)) };
    shown.map_err(io::Error::from)
}

// A file that cannot go to the Recycle Bin is deleted only after Windows asks.
fn recycle(path: &Path, owner: HWND) -> io::Result<bool> {
    fits_the_shell(path)?;
    let names: Vec<u16> = path.as_os_str().encode_wide().chain([0, 0]).collect();
    let flags =
        FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_WANTNUKEWARNING | FOF_NOERRORUI | FOF_SILENT;
    let mut operation = SHFILEOPSTRUCTW {
        hwnd: owner,
        wFunc: FO_DELETE,
        pFrom: PCWSTR(names.as_ptr()),
        fFlags: u16::try_from(flags.0).unwrap_or(0),
        ..Default::default()
    };
    let code = unsafe { SHFileOperationW(&mut operation) };
    if code != 0 {
        return Err(io::Error::other(format!(
            "the file could not be moved to the Recycle Bin (shell error {code:#x})"
        )));
    }
    Ok(!operation.fAnyOperationsAborted.as_bool())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn path(units: usize) -> PathBuf {
        PathBuf::from(format!(r"C:\{}", "a".repeat(units - 3)))
    }

    #[test]
    fn a_path_the_shell_cannot_take_is_refused_in_windows_words() {
        assert!(fits_the_shell(&path(259)).is_ok());
        let error = fits_the_shell(&path(260)).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(206));
        assert!(fits_the_shell(&path(400)).is_err());
    }

    #[test]
    fn a_path_is_measured_in_utf16_units() {
        let emoji = PathBuf::from(format!(r"C:\{}{}", "a".repeat(255), '\u{1F600}'));
        assert!(fits_the_shell(&emoji).is_err());
        let accented = PathBuf::from(format!(r"C:\{}", "ç".repeat(256)));
        assert!(fits_the_shell(&accented).is_ok());
    }
}
