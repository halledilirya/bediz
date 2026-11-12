use std::cell::Cell;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, S_FALSE, S_OK, WPARAM};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Controls::{
    TASKDIALOG_BUTTON, TASKDIALOG_FLAGS, TASKDIALOG_MESSAGES, TASKDIALOG_NOTIFICATIONS,
    TASKDIALOGCONFIG, TDE_CONTENT, TDF_ALLOW_DIALOG_CANCELLATION, TDF_RTL_LAYOUT,
    TDF_SHOW_PROGRESS_BAR, TDM_CLICK_BUTTON, TDM_ENABLE_BUTTON, TDM_SET_ELEMENT_TEXT,
    TDM_SET_PROGRESS_BAR_POS, TDM_SET_PROGRESS_BAR_RANGE, TDN_BUTTON_CLICKED, TDN_CREATED,
    TDN_DESTROYED,
};
use windows::Win32::UI::WindowsAndMessaging::{IDCANCEL, PostMessageW, SendMessageW};
use windows::core::{BOOL, Error, HRESULT, HSTRING, PCWSTR, Result, s, w};

const ACCEPT: i32 = 100;
const DECLINE: i32 = 101;
const WHEN_SAVED: i32 = 110;
const DISCARD: i32 = 111;
const STAY: i32 = 112;
const PERCENT_RANGE: isize = 100 << 16;

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

pub struct QuitQuestion {
    pub title: String,
    pub heading: String,
    pub body: String,
    pub waiting: String,
    pub when_saved: String,
    pub discard: String,
    pub stay: String,
    pub right_to_left: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuitChoice {
    WhenSaved,
    Discard,
    Stay,
}

#[derive(Clone, Default)]
pub struct QuitDialog(Arc<Shown>);

#[derive(Default)]
struct Shown {
    window: AtomicIsize,
    percent: AtomicUsize,
    closing: AtomicBool,
}

impl QuitDialog {
    pub fn is(&self, other: &QuitDialog) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn show_percent(&self, percent: u8) {
        self.0
            .percent
            .store(usize::from(percent), Ordering::Release);
        self.post(TDM_SET_PROGRESS_BAR_POS, usize::from(percent));
    }

    pub fn close(&self) {
        self.0.closing.store(true, Ordering::Release);
        self.post(TDM_CLICK_BUTTON, IDCANCEL.0 as usize);
    }

    fn post(&self, message: TASKDIALOG_MESSAGES, value: usize) {
        let window = self.0.window.load(Ordering::Acquire);
        if window != 0 {
            post(HWND(window as *mut c_void), message, value);
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Close(Option<QuitChoice>),
    Wait,
    StayOpen,
}

struct Asking<'a> {
    dialog: &'a QuitDialog,
    waiting_text: HSTRING,
    waiting: Cell<bool>,
    chosen: &'a dyn Fn(QuitChoice),
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

pub fn ask_to_quit(
    question: &QuitQuestion,
    dialog: &QuitDialog,
    chosen: &dyn Fn(QuitChoice),
) -> Result<()> {
    let show = task_dialog()?;
    let [title, heading, body, waiting, when_saved, discard, stay] = [
        &question.title,
        &question.heading,
        &question.body,
        &question.waiting,
        &question.when_saved,
        &question.discard,
        &question.stay,
    ]
    .map(|text| HSTRING::from(text.as_str()));
    let buttons = [
        (WHEN_SAVED, &when_saved),
        (DISCARD, &discard),
        (STAY, &stay),
    ]
    .map(|(id, text)| TASKDIALOG_BUTTON {
        nButtonID: id,
        pszButtonText: PCWSTR(text.as_ptr()),
    });
    let asking = Asking {
        dialog,
        waiting_text: waiting,
        waiting: Cell::new(false),
        chosen,
    };
    let config = TASKDIALOGCONFIG {
        cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
        dwFlags: flags(question.right_to_left) | TDF_SHOW_PROGRESS_BAR,
        pszWindowTitle: PCWSTR(title.as_ptr()),
        pszMainInstruction: PCWSTR(heading.as_ptr()),
        pszContent: PCWSTR(body.as_ptr()),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: WHEN_SAVED,
        pfCallback: Some(on_quit_note),
        lpCallbackData: &asking as *const Asking as isize,
        ..Default::default()
    };
    unsafe {
        show(
            &config,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    }
    .ok()
}

unsafe extern "system" fn on_quit_note(
    window: HWND,
    note: TASKDIALOG_NOTIFICATIONS,
    wparam: WPARAM,
    _: LPARAM,
    data: isize,
) -> HRESULT {
    let Some(asking) = (unsafe { (data as *const Asking).as_ref() }) else {
        return S_OK;
    };
    let shown = &asking.dialog.0;
    match note {
        TDN_CREATED => {
            shown.window.store(window.0 as isize, Ordering::Release);
            send(window, TDM_SET_PROGRESS_BAR_RANGE, 0, PERCENT_RANGE);
            let percent = shown.percent.load(Ordering::Acquire);
            send(window, TDM_SET_PROGRESS_BAR_POS, percent, 0);
            if shown.closing.load(Ordering::Acquire) {
                post(window, TDM_CLICK_BUTTON, IDCANCEL.0 as usize);
            }
            S_OK
        }
        TDN_DESTROYED => {
            shown.window.store(0, Ordering::Release);
            S_OK
        }
        TDN_BUTTON_CLICKED => {
            let closing = shown.closing.load(Ordering::Acquire);
            match answer(wparam.0 as i32, asking.waiting.get(), closing) {
                Answer::Close(choice) => {
                    if let Some(choice) = choice {
                        (asking.chosen)(choice);
                    }
                    S_OK
                }
                Answer::Wait => {
                    asking.waiting.set(true);
                    send(window, TDM_ENABLE_BUTTON, WHEN_SAVED as usize, 0);
                    let text = asking.waiting_text.as_ptr() as isize;
                    send(window, TDM_SET_ELEMENT_TEXT, TDE_CONTENT.0 as usize, text);
                    (asking.chosen)(QuitChoice::WhenSaved);
                    S_FALSE
                }
                Answer::StayOpen => S_FALSE,
            }
        }
        _ => S_OK,
    }
}

fn answer(button: i32, waiting: bool, closing: bool) -> Answer {
    if closing {
        return Answer::Close(None);
    }
    match button {
        WHEN_SAVED if waiting => Answer::StayOpen,
        WHEN_SAVED => Answer::Wait,
        DISCARD => Answer::Close(Some(QuitChoice::Discard)),
        _ => Answer::Close(Some(QuitChoice::Stay)),
    }
}

fn send(window: HWND, message: TASKDIALOG_MESSAGES, wparam: usize, lparam: isize) {
    unsafe {
        SendMessageW(
            window,
            message.0 as u32,
            Some(WPARAM(wparam)),
            Some(LPARAM(lparam)),
        )
    };
}

fn post(window: HWND, message: TASKDIALOG_MESSAGES, wparam: usize) {
    let _ = unsafe { PostMessageW(Some(window), message.0 as u32, WPARAM(wparam), LPARAM(0)) };
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

    #[test]
    fn closing_when_saved_keeps_the_question_open_until_the_gif_is_saved() {
        assert_eq!(answer(WHEN_SAVED, false, false), Answer::Wait);
        assert_eq!(answer(WHEN_SAVED, true, false), Answer::StayOpen);
    }

    #[test]
    fn the_gif_can_be_deleted_before_or_while_waiting() {
        let discard = Answer::Close(Some(QuitChoice::Discard));
        assert_eq!(answer(DISCARD, false, false), discard);
        assert_eq!(answer(DISCARD, true, false), discard);
    }

    #[test]
    fn cancel_esc_and_the_close_box_keep_bediz_open_before_or_while_waiting() {
        let stay = Answer::Close(Some(QuitChoice::Stay));
        for waiting in [false, true] {
            assert_eq!(answer(STAY, waiting, false), stay);
            assert_eq!(answer(IDCANCEL.0, waiting, false), stay);
        }
    }

    #[test]
    fn a_question_closed_by_bediz_chooses_nothing() {
        for button in [WHEN_SAVED, DISCARD, STAY, IDCANCEL.0] {
            assert_eq!(answer(button, false, true), Answer::Close(None));
        }
    }
}
