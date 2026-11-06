use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Gdi::{COLOR_HIGHLIGHT, COLOR_WINDOW, GetSysColor};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colours {
    pub ink: D2D1_COLOR_F,
    pub ground: D2D1_COLOR_F,
}

pub fn contrast_colours() -> Option<Colours> {
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
    let on = read.is_ok() && contrast.dwFlags.0 & HCF_HIGHCONTRASTON.0 != 0;
    on.then(|| Colours {
        ink: colour(unsafe { GetSysColor(COLOR_HIGHLIGHT) }),
        ground: colour(unsafe { GetSysColor(COLOR_WINDOW) }),
    })
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
