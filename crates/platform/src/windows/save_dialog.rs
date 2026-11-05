use std::cell::Cell;
use std::ffi::{OsString, c_void};
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use windows::Win32::Foundation::{ERROR_CANCELLED, HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize, IBindCtx,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FDE_OVERWRITE_RESPONSE, FDE_SHAREVIOLATION_RESPONSE, FDEOR_DEFAULT, FDESVR_DEFAULT,
    FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FileSaveDialog, IFileDialog, IFileDialogEvents,
    IFileDialogEvents_Impl, IFileSaveDialog, IShellItem, SHCreateItemFromParsingName,
    SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::{HSTRING, Result, w};
use windows_core::{ComObject, Ref, implement};

use super::runtime::WM_SAVE_AS;
use super::save::{self, FileId};

pub struct Answer {
    pub job: u64,
    pub chosen: Result<Option<Chosen>>,
}

pub struct Chosen {
    pub path: PathBuf,
    pub replace: Option<FileId>,
}

pub type Answers = Arc<Mutex<Option<Answer>>>;

pub struct Suggestion {
    pub name: String,
    pub folder: Option<PathBuf>,
}

// The common dialogs need a single-threaded apartment; the platform thread is multithreaded.
pub fn ask(
    window: HWND,
    answers: &Answers,
    job: u64,
    suggestion: Suggestion,
) -> std::io::Result<()> {
    let answers = Arc::clone(answers);
    let window = window.0 as isize;
    std::thread::Builder::new()
        .name("bediz-save-as".to_owned())
        .spawn(move || {
            let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
            let chosen = show(&suggestion);
            if com {
                unsafe { CoUninitialize() };
            }
            *answers.lock().unwrap_or_else(PoisonError::into_inner) = Some(Answer { job, chosen });
            let window = HWND(window as *mut c_void);
            let _ = unsafe { PostMessageW(Some(window), WM_SAVE_AS, WPARAM(0), LPARAM(0)) };
        })?;
    Ok(())
}

fn show(suggestion: &Suggestion) -> Result<Option<Chosen>> {
    let dialog: IFileSaveDialog =
        unsafe { CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER) }?;
    let types = [COMDLG_FILTERSPEC {
        pszName: w!("PNG"),
        pszSpec: w!("*.png"),
    }];
    unsafe {
        dialog.SetFileTypes(&types)?;
        dialog.SetDefaultExtension(w!("png"))?;
        dialog.SetOptions(dialog.GetOptions()? | FOS_OVERWRITEPROMPT | FOS_FORCEFILESYSTEM)?;
        dialog.SetFileName(&HSTRING::from(suggestion.name.as_str()))?;
    }
    match &suggestion.folder {
        Some(folder) => {
            if let Some(item) = shell_item(folder) {
                unsafe { dialog.SetFolder(&item) }?;
            }
        }
        None => {
            if let Some(item) = save::screenshots_folder()
                .ok()
                .and_then(|folder| shell_item(&folder))
            {
                unsafe { dialog.SetDefaultFolder(&item) }?;
            }
        }
    }
    let watch = ComObject::new(OverwriteWatch {
        asked: Cell::new(None),
    });
    let events: IFileDialogEvents = watch.to_interface();
    let cookie = unsafe { dialog.Advise(&events) }?;
    let shown = unsafe { dialog.Show(None) };
    let _ = unsafe { dialog.Unadvise(cookie) };
    match shown {
        Ok(()) => {}
        Err(error) if error.code() == ERROR_CANCELLED.to_hresult() => return Ok(None),
        Err(error) => return Err(error),
    }
    let chosen = file_path(&unsafe { dialog.GetResult() }?)?;
    let replace = confirmed(watch.asked.take(), &chosen, save::file_id(&chosen).ok());
    Ok(Some(Chosen {
        path: chosen,
        replace,
    }))
}

// OnOverwrite fires for a No too, and after a No the dialog only closes once the name is free.
fn confirmed(
    asked: Option<(PathBuf, FileId)>,
    chosen: &Path,
    now: Option<FileId>,
) -> Option<FileId> {
    asked.and_then(|(path, file)| (path == chosen && now == Some(file)).then_some(file))
}

#[implement(IFileDialogEvents)]
struct OverwriteWatch {
    asked: Cell<Option<(PathBuf, FileId)>>,
}

impl IFileDialogEvents_Impl for OverwriteWatch_Impl {
    fn OnFileOk(&self, _: Ref<IFileDialog>) -> Result<()> {
        Ok(())
    }

    fn OnFolderChanging(&self, _: Ref<IFileDialog>, _: Ref<IShellItem>) -> Result<()> {
        Ok(())
    }

    fn OnFolderChange(&self, _: Ref<IFileDialog>) -> Result<()> {
        Ok(())
    }

    fn OnSelectionChange(&self, _: Ref<IFileDialog>) -> Result<()> {
        Ok(())
    }

    fn OnShareViolation(
        &self,
        _: Ref<IFileDialog>,
        _: Ref<IShellItem>,
    ) -> Result<FDE_SHAREVIOLATION_RESPONSE> {
        Ok(FDESVR_DEFAULT)
    }

    fn OnTypeChange(&self, _: Ref<IFileDialog>) -> Result<()> {
        Ok(())
    }

    fn OnOverwrite(
        &self,
        _: Ref<IFileDialog>,
        item: Ref<IShellItem>,
    ) -> Result<FDE_OVERWRITE_RESPONSE> {
        let asked = item
            .ok()
            .ok()
            .and_then(|item| file_path(item).ok())
            .and_then(|path| save::file_id(&path).ok().map(|file| (path, file)));
        self.asked.set(asked);
        Ok(FDEOR_DEFAULT)
    }
}

fn file_path(item: &IShellItem) -> Result<PathBuf> {
    let path = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }?;
    let chosen = PathBuf::from(OsString::from_wide(unsafe { path.as_wide() }));
    unsafe { CoTaskMemFree(Some(path.0.cast())) };
    Ok(chosen)
}

fn shell_item(folder: &Path) -> Option<IShellItem> {
    unsafe {
        SHCreateItemFromParsingName::<_, _, IShellItem>(
            &HSTRING::from(folder.as_os_str()),
            None::<&IBindCtx>,
        )
    }
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_file_asked_about_may_be_replaced() {
        let file = FileId::for_tests(1);
        let asked = Some((PathBuf::from(r"C:\Pictures\notes.png"), file));
        let notes = Path::new(r"C:\Pictures\notes.png");
        assert_eq!(confirmed(asked.clone(), notes, Some(file)), Some(file));
        assert_eq!(
            confirmed(asked, Path::new(r"C:\Pictures\other.png"), Some(file)),
            None
        );
        assert_eq!(confirmed(None, notes, Some(file)), None);
    }

    #[test]
    fn a_question_answered_no_does_not_carry_over_to_a_free_name() {
        let file = FileId::for_tests(1);
        let asked = Some((PathBuf::from(r"C:\Pictures\notes.png"), file));
        let notes = Path::new(r"C:\Pictures\notes.png");
        assert_eq!(confirmed(asked.clone(), notes, None), None);
        assert_eq!(confirmed(asked, notes, Some(FileId::for_tests(2))), None);
    }
}
