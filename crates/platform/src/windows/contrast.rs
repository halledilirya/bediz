//! Reads whether high contrast is on and its colours, and whether apps follow the dark theme.

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Gdi::{COLOR_HIGHLIGHT, COLOR_WINDOW, GetSysColor};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::w;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colours {
    pub ink: D2D1_COLOR_F,
    pub ground: D2D1_COLOR_F,
}

pub fn contrast_colours() -> Option<Colours> {
    high_contrast().then(|| Colours {
        ink: colour(unsafe { GetSysColor(COLOR_HIGHLIGHT) }),
        ground: colour(unsafe { GetSysColor(COLOR_WINDOW) }),
    })
}

// The value tao reads for a window that follows Windows, so this matches its title bar.
pub fn apps_use_dark_mode() -> bool {
    let mut light = 1u32;
    let mut size = size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut light as *mut u32).cast()),
            Some(&mut size),
        )
    };
    status == ERROR_SUCCESS && light == 0 && !high_contrast()
}

pub fn high_contrast() -> bool {
    let mut contrast = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cbSize,
            Some((&mut contrast as *mut HIGHCONTRASTW).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_ok() && contrast.dwFlags.0 & HCF_HIGHCONTRASTON.0 != 0
}

fn colour(rgb: u32) -> D2D1_COLOR_F {
    let channel = |shift: u32| ((rgb >> shift) & 0xFF) as f32 / 255.0;
    D2D1_COLOR_F {
        r: channel(0),
        g: channel(8),
        b: channel(16),
        a: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_system_colour_is_red_in_its_low_byte() {
        let teal = colour(0x00F0_E38E);
        assert!((teal.r - 0x8E as f32 / 255.0).abs() < f32::EPSILON);
        assert!((teal.g - 0xE3 as f32 / 255.0).abs() < f32::EPSILON);
        assert!((teal.b - 0xF0 as f32 / 255.0).abs() < f32::EPSILON);
        assert!((teal.a - 1.0).abs() < f32::EPSILON);
    }
}
