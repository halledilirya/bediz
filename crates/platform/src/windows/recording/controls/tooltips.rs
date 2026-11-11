use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::UI::Controls::{
    ICC_BAR_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, TOOLTIPS_CLASSW, TTF_IDISHWND,
    TTF_RTLREADING, TTF_SUBCLASS, TTM_ADDTOOLW, TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DestroyWindow, SendMessageW, SetWindowDisplayAffinity,
    WDA_EXCLUDEFROMCAPTURE, WINDOW_STYLE, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, PWSTR, Result};

pub(super) fn add(
    instance: HINSTANCE,
    owner: HWND,
    right_to_left: bool,
    tools: &[(HWND, &str)],
) -> Result<Vec<Vec<u16>>> {
    let classes = INITCOMMONCONTROLSEX {
        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_BAR_CLASSES,
    };
    let _ = unsafe { InitCommonControlsEx(&classes) };
    let tip = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST,
            TOOLTIPS_CLASSW,
            PCWSTR::null(),
            WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP | TTS_NOPREFIX),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            Some(owner),
            None,
            Some(instance),
            None,
        )
    }?;
    if let Err(error) = unsafe { SetWindowDisplayAffinity(tip, WDA_EXCLUDEFROMCAPTURE) } {
        let _ = unsafe { DestroyWindow(tip) };
        return Err(error);
    }
    let mut flags = TTF_IDISHWND | TTF_SUBCLASS;
    if right_to_left {
        flags |= TTF_RTLREADING;
    }
    let mut texts = Vec::with_capacity(tools.len());
    for &(tool, text) in tools {
        let mut wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let mut info = TTTOOLINFOW {
            cbSize: size_of::<TTTOOLINFOW>() as u32,
            uFlags: flags,
            hwnd: owner,
            uId: tool.0 as usize,
            lpszText: PWSTR(wide.as_mut_ptr()),
            ..Default::default()
        };
        unsafe {
            SendMessageW(
                tip,
                TTM_ADDTOOLW,
                Some(WPARAM(0)),
                Some(LPARAM(&mut info as *mut TTTOOLINFOW as isize)),
            )
        };
        texts.push(wide);
    }
    Ok(texts)
}
