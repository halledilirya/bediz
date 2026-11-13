//! Reads each monitor's advanced colour state and SDR white level, which set how its linear frames
//! are scaled to sRGB.

use std::num::NonZeroU32;

use bediz_core::capture::Pixels;
use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_DEVICE_INFO_TYPE,
    DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SDR_WHITE_LEVEL, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DisplayConfigGetDeviceInfo,
    GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
};
use windows::Win32::Foundation::{E_FAIL, ERROR_INSUFFICIENT_BUFFER, LUID, WIN32_ERROR};
use windows::core::{Error, Result};

const ADVANCED_COLOR_ENABLED: u32 = 1 << 1;
const WIDE_COLOR_ENFORCED: u32 = 1 << 2;
const ATTEMPTS: usize = 3;

pub type DeviceName = [u16; 32];

type Sources = Result<Vec<(DeviceName, Result<Pixels>)>>;

#[derive(Default)]
pub struct WhiteLevels {
    read: Option<Sources>,
}

impl WhiteLevels {
    pub fn pixels(&mut self, name: &DeviceName) -> Result<Pixels> {
        lookup(self.read.get_or_insert_with(read), name).map_err(|error| {
            Error::new(
                error.code(),
                format!(
                    "the monitor's SDR white level is unknown: {}",
                    error.message()
                ),
            )
        })
    }
}

fn lookup(sources: &Sources, name: &DeviceName) -> Result<Pixels> {
    sources
        .as_ref()
        .map_err(Clone::clone)?
        .iter()
        .find(|(source, _)| same_name(source, name))
        .map_or_else(
            || Err(Error::new(E_FAIL, "no active display path shows it")),
            |(_, pixels)| pixels.clone(),
        )
}

fn read() -> Sources {
    active_paths()?
        .iter()
        .map(|path| Ok((source_name(path)?, pixels(path))))
        .collect()
}

// The configuration can change between the size query and the query itself.
fn active_paths() -> Result<Vec<DISPLAYCONFIG_PATH_INFO>> {
    for _ in 0..ATTEMPTS {
        let (mut path_count, mut mode_count) = (0, 0);
        unsafe {
            GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
        }
        .ok()?;
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
        let queried = unsafe {
            QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut path_count,
                paths.as_mut_ptr(),
                &mut mode_count,
                modes.as_mut_ptr(),
                None,
            )
        };
        if queried == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        queried.ok()?;
        paths.truncate(path_count as usize);
        return Ok(paths);
    }
    Err(ERROR_INSUFFICIENT_BUFFER.into())
}

fn source_name(path: &DISPLAYCONFIG_PATH_INFO) -> Result<DeviceName> {
    let mut request = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
        header: header::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>(
            DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
            path.sourceInfo.adapterId,
            path.sourceInfo.id,
        ),
        ..Default::default()
    };
    device_info(&mut request.header)?;
    Ok(request.viewGdiDeviceName)
}

fn pixels(path: &DISPLAYCONFIG_PATH_INFO) -> Result<Pixels> {
    let (adapter, id) = (path.targetInfo.adapterId, path.targetInfo.id);
    let mut colour = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
        header: header::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>(
            DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
            adapter,
            id,
        ),
        ..Default::default()
    };
    device_info(&mut colour.header)?;
    classify(unsafe { colour.Anonymous.value }, || {
        let mut white = DISPLAYCONFIG_SDR_WHITE_LEVEL {
            header: header::<DISPLAYCONFIG_SDR_WHITE_LEVEL>(
                DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
                adapter,
                id,
            ),
            SDRWhiteLevel: 0,
        };
        device_info(&mut white.header).map(|()| white.SDRWhiteLevel)
    })
}

fn classify(state: u32, white_level: impl FnOnce() -> Result<u32>) -> Result<Pixels> {
    if state & ADVANCED_COLOR_ENABLED == 0 || state & WIDE_COLOR_ENFORCED != 0 {
        return Ok(Pixels::Linear);
    }
    NonZeroU32::new(white_level()?)
        .map(|sdr_white_level| Pixels::Hdr { sdr_white_level })
        .ok_or_else(|| Error::new(E_FAIL, "Windows gave 0 for it"))
}

fn device_info(header: &mut DISPLAYCONFIG_DEVICE_INFO_HEADER) -> Result<()> {
    WIN32_ERROR(unsafe { DisplayConfigGetDeviceInfo(header) } as u32).ok()
}

fn header<T>(
    kind: DISPLAYCONFIG_DEVICE_INFO_TYPE,
    adapter: LUID,
    id: u32,
) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER {
        r#type: kind,
        size: size_of::<T>() as u32,
        adapterId: adapter,
        id,
    }
}

fn same_name(left: &DeviceName, right: &DeviceName) -> bool {
    trimmed(left) == trimmed(right)
}

fn trimmed(name: &DeviceName) -> &[u16] {
    name.split(|&unit| unit == 0).next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::ERROR_INVALID_PARAMETER;
    use windows::core::HRESULT;

    use super::*;

    const SUPPORTED: u32 = 1;
    const HDR: u32 = SUPPORTED | ADVANCED_COLOR_ENABLED;

    fn name(text: &str, filler: u16) -> DeviceName {
        let mut name = [filler; 32];
        for (unit, character) in name.iter_mut().zip(text.encode_utf16().chain([0])) {
            *unit = character;
        }
        name
    }

    fn refused<T>() -> Result<T> {
        Err(ERROR_INVALID_PARAMETER.into())
    }

    fn hdr(level: u32) -> Option<Pixels> {
        NonZeroU32::new(level).map(|sdr_white_level| Pixels::Hdr { sdr_white_level })
    }

    fn code(pixels: Result<Pixels>) -> Option<HRESULT> {
        pixels.err().map(|error| error.code())
    }

    #[test]
    fn an_hdr_monitor_needs_its_white_level() {
        assert_eq!(classify(HDR, || Ok(2500)).ok(), hdr(2500));
        assert_eq!(
            code(classify(HDR, refused)),
            Some(ERROR_INVALID_PARAMETER.to_hresult())
        );
        assert_eq!(code(classify(HDR, || Ok(0))), Some(E_FAIL));
    }

    #[test]
    fn other_monitors_need_no_white_level() {
        assert_eq!(classify(SUPPORTED, refused).ok(), Some(Pixels::Linear));
        assert_eq!(
            classify(HDR | WIDE_COLOR_ENFORCED, refused).ok(),
            Some(Pixels::Linear)
        );
    }

    #[test]
    fn a_monitor_the_display_paths_do_not_describe_is_an_error() {
        let display = name(r"\\.\DISPLAY1", 0);
        let other = name(r"\\.\DISPLAY2", 0);
        let pixels = |sources: Sources| {
            WhiteLevels {
                read: Some(sources),
            }
            .pixels(&display)
        };
        let refusal = Some(ERROR_INVALID_PARAMETER.to_hresult());
        assert_eq!(code(pixels(refused())), refusal);
        assert_eq!(code(pixels(Ok(vec![(display, refused())]))), refusal);
        assert_eq!(
            code(pixels(Ok(vec![(other, Ok(Pixels::Linear))]))),
            Some(E_FAIL)
        );
        assert_eq!(
            pixels(Ok(vec![(other, refused()), (display, Ok(Pixels::Linear))])).ok(),
            Some(Pixels::Linear)
        );
    }

    #[test]
    fn names_compare_up_to_their_end() {
        assert!(same_name(
            &name(r"\\.\DISPLAY1", 0),
            &name(r"\\.\DISPLAY1", 7)
        ));
        assert!(!same_name(
            &name(r"\\.\DISPLAY1", 0),
            &name(r"\\.\DISPLAY12", 0)
        ));
    }
}
