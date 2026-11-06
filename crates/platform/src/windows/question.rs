use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Controls::{
    TASKDIALOG_BUTTON, TASKDIALOG_FLAGS, TASKDIALOGCONFIG, TDF_ALLOW_DIALOG_CANCELLATION,
    TDF_RTL_LAYOUT,
};
use windows::core::{BOOL, Error, HRESULT, HSTRING, PCWSTR, Result, s, w};

const ACCEPT: i32 = 100;
const DECLINE: i32 = 101;

type TaskDialogIndirect =
    unsafe extern "system" fn(*const TASKDIALOGCONFIG, *mut i32, *mut i32, *mut BOOL) -> HRESULT;

pub struct Question {
    pub title: String,
    pub heading: String,
    pub body: String,
    pub accept: String,
    pub decline: String,
    pub never_again: String,
    pub right_to_left: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    pub accepted: bool,
    pub never_again: bool,
}

pub fn ask(question: &Question) -> Result<Reply> {
    let show = task_dialog()?;
    let [title, heading, body, accept, decline, never_again] = [
        &question.title,
        &question.heading,
        &question.body,
        &question.accept,
        &question.decline,
        &question.never_again,
    ]
    .map(|text| HSTRING::from(text.as_str()));
    let buttons = [
        TASKDIALOG_BUTTON {
            nButtonID: ACCEPT,
            pszButtonText: PCWSTR(accept.as_ptr()),
        },
        TASKDIALOG_BUTTON {
            nButtonID: DECLINE,
            pszButtonText: PCWSTR(decline.as_ptr()),
        },
    ];
    let config = TASKDIALOGCONFIG {
        cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
        dwFlags: flags(question.right_to_left),
        pszWindowTitle: PCWSTR(title.as_ptr()),
        pszMainInstruction: PCWSTR(heading.as_ptr()),
        pszContent: PCWSTR(body.as_ptr()),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: ACCEPT,
        pszVerificationText: PCWSTR(never_again.as_ptr()),
        ..Default::default()
    };
    let mut pressed = 0;
    let mut checked = BOOL(0);
    unsafe { show(&config, &mut pressed, std::ptr::null_mut(), &mut checked) }.ok()?;
    Ok(Reply {
        accepted: pressed == ACCEPT,
        never_again: checked.as_bool(),
    })
}

fn flags(right_to_left: bool) -> TASKDIALOG_FLAGS {
    if right_to_left {
        TDF_ALLOW_DIALOG_CANCELLATION | TDF_RTL_LAYOUT
    } else {
        TDF_ALLOW_DIALOG_CANCELLATION
    }
}

// Only comctl32 6, chosen by the app's manifest, has it; linking it would stop test binaries loading.
fn task_dialog() -> Result<TaskDialogIndirect> {
    let module = unsafe { LoadLibraryW(w!("comctl32.dll")) }?;
    let found = unsafe { GetProcAddress(module, s!("TaskDialogIndirect")) }
        .ok_or_else(Error::from_thread)?;
    Ok(unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, TaskDialogIndirect>(found)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_right_to_left_question_is_mirrored_and_can_still_be_cancelled() {
        assert_eq!(flags(false), TDF_ALLOW_DIALOG_CANCELLATION);
        assert_eq!(flags(true), TDF_ALLOW_DIALOG_CANCELLATION | TDF_RTL_LAYOUT);
    }
}
