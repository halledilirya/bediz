//! Shows the system folder picker and tells whether a folder lies inside a cloud-synced folder.

use std::ffi::c_void;
use std::io;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog,
};

use super::save_dialog::{file_path, shell_item};

const ONEDRIVE_ROOTS: [&str; 3] = ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"];

pub fn synced_by_onedrive(folder: &Path) -> bool {
    ONEDRIVE_ROOTS
        .iter()
        .filter_map(std::env::var_os)
        .any(|root| is_inside(folder, Path::new(&root)))
}

fn is_inside(folder: &Path, root: &Path) -> bool {
    let plain = |path: &Path| {
        path.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    let (folder, root) = (plain(folder), plain(root));
    !root.is_empty()
        && folder
            .strip_prefix(&root)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('\\'))
}

// The common dialogs need a single-threaded apartment, so the dialog gets a thread of its own.
pub fn pick_folder(owner: isize, start: Option<PathBuf>) -> io::Result<Option<PathBuf>> {
    std::thread::Builder::new()
        .name("bediz-folder".to_owned())
        .spawn(move || {
            let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
            let picked = show(HWND(owner as *mut c_void), start.as_deref());
            if com {
                unsafe { CoUninitialize() };
            }
            picked
        })?
        .join()
        .unwrap_or_else(|_| Err(io::Error::other("the folder dialog stopped")))
}

fn show(owner: HWND, start: Option<&Path>) -> io::Result<Option<PathBuf>> {
    let dialog: IFileOpenDialog =
        unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }?;
    unsafe { dialog.SetOptions(dialog.GetOptions()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM) }?;
    if let Some(item) = start.and_then(shell_item) {
        unsafe { dialog.SetFolder(&item) }?;
    }
    match unsafe { dialog.Show(Some(owner)) } {
        Ok(()) => {}
        Err(error) if error.code() == ERROR_CANCELLED.to_hresult() => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    Ok(Some(file_path(&unsafe { dialog.GetResult() }?)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(folder: &str, root: &str) -> bool {
        is_inside(Path::new(folder), Path::new(root))
    }

    #[test]
    fn a_folder_under_onedrive_is_found_whatever_the_case() {
        assert!(inside(
            r"C:\Users\Ada\OneDrive\Pictures\Screenshots",
            r"C:\Users\Ada\OneDrive"
        ));
        assert!(inside(r"c:\users\ada\onedrive", r"C:\Users\Ada\OneDrive\"));
        assert!(inside(r"C:\Users\Ada\OneDrive\x", r"C:/Users/Ada/OneDrive"));
    }

    #[test]
    fn a_folder_beside_onedrive_is_not_inside_it() {
        assert!(!inside(
            r"C:\Users\Ada\OneDrive - Work\Pictures",
            r"C:\Users\Ada\OneDrive"
        ));
        assert!(!inside(r"C:\Users\Ada\Pictures", r"C:\Users\Ada\OneDrive"));
        assert!(!inside(r"C:\Users\Ada\Pictures", ""));
    }
}
