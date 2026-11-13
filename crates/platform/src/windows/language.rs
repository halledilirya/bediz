//! Reads the user's preferred Windows display languages, in order.

use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
use windows::core::PWSTR;

pub fn preferred_languages() -> Vec<String> {
    let mut count = 0;
    let mut length = 0;
    if unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut length) }
        .is_err()
    {
        return Vec::new();
    }
    let mut buffer = vec![0u16; length as usize];
    if unsafe {
        GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            Some(PWSTR(buffer.as_mut_ptr())),
            &mut length,
        )
    }
    .is_err()
    {
        return Vec::new();
    }
    buffer
        .split(|unit| *unit == 0)
        .filter(|name| !name.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}
