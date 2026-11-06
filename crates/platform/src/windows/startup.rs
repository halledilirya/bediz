use std::ffi::OsStr;
use std::io;
use std::iter;
use std::os::windows::ffi::OsStrExt;

use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::{PCWSTR, w};

const RUN: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const NAME: PCWSTR = w!("Bediz");

pub fn start_with_windows(on: bool) -> io::Result<()> {
    let current = read()?;
    if !on {
        if current.is_none() {
            return Ok(());
        }
        let deleted = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, NAME) };
        if deleted != ERROR_FILE_NOT_FOUND {
            deleted.ok()?;
        }
        return Ok(());
    }
    let wanted = quoted(std::env::current_exe()?.as_os_str());
    if current.as_deref() == Some(wanted.as_slice()) {
        return Ok(());
    }
    let data: Vec<u16> = wanted.iter().copied().chain(iter::once(0)).collect();
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN,
            NAME,
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            u32::try_from(data.len() * 2).unwrap_or(u32::MAX),
        )
    }
    .ok()?;
    Ok(())
}

fn read() -> io::Result<Option<Vec<u16>>> {
    let mut size = 0u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN,
            NAME,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    status.ok()?;
    let mut value = vec![0u16; (size as usize).div_ceil(2)];
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN,
            NAME,
            RRF_RT_REG_SZ,
            None,
            Some(value.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    status.ok()?;
    value.truncate((size as usize / 2).saturating_sub(1));
    Ok(Some(value))
}

fn quoted(path: &OsStr) -> Vec<u16> {
    let quote = u16::from(b'"');
    iter::once(quote)
        .chain(path.encode_wide())
        .chain(iter::once(quote))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_is_quoted_so_spaces_in_the_path_work() {
        let expected: Vec<u16> = r#""C:\Program Files\Bediz\bediz.exe""#.encode_utf16().collect();
        assert_eq!(
            quoted(OsStr::new(r"C:\Program Files\Bediz\bediz.exe")),
            expected
        );
    }
}
