use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, KEY_QUERY_VALUE, REG_DWORD, REG_NOTIFY_CHANGE_LAST_SET,
    RRF_RT_REG_DWORD, RegCloseKey, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW,
    RegSetKeyValueW,
};
use windows::Win32::System::Threading::CreateEventW;
use windows::core::{PCWSTR, Result, w};

const KEYBOARD: PCWSTR = w!("Control Panel\\Keyboard");
const SNIPPING_TOOL: PCWSTR = w!("PrintScreenKeyForSnippingEnabled");

pub fn read() -> Option<u32> {
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            KEYBOARD,
            SNIPPING_TOOL,
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then_some(value)
}

pub fn turn_off_snipping_tool() -> Result<()> {
    let off = 0u32;
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            KEYBOARD,
            SNIPPING_TOOL,
            REG_DWORD.0,
            Some((&off as *const u32).cast()),
            size_of::<u32>() as u32,
        )
    }
    .ok()
}

pub struct Watch {
    key: HKEY,
    event: HANDLE,
}

impl Watch {
    pub fn start() -> Result<Self> {
        let mut key = HKEY::default();
        unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                KEYBOARD,
                None,
                KEY_NOTIFY | KEY_QUERY_VALUE,
                &mut key,
            )
        }
        .ok()?;
        let event = match unsafe { CreateEventW(None, false, false, None) } {
            Ok(event) => event,
            Err(error) => {
                let _ = unsafe { RegCloseKey(key) };
                return Err(error);
            }
        };
        let watch = Self { key, event };
        watch.arm()?;
        Ok(watch)
    }

    pub fn arm(&self) -> Result<()> {
        unsafe {
            RegNotifyChangeKeyValue(
                self.key,
                false,
                REG_NOTIFY_CHANGE_LAST_SET,
                Some(self.event),
                true,
            )
        }
        .ok()
    }

    pub fn event(&self) -> HANDLE {
        self.event
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.key);
            let _ = CloseHandle(self.event);
        }
    }
}
