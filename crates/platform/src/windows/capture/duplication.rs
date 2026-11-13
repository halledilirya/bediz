//! Desktop Duplication of one monitor: acquiring and releasing its frames, reopening it when lost,
//! and reporting whether the pointer is kept apart from the image.

use std::time::Instant;

use bediz_core::capture::Pixels;
use windows::Win32::Foundation::{E_FAIL, E_POINTER};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_TEXTURE2D_DESC, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_INVALID_CALL, DXGI_ERROR_WAIT_TIMEOUT,
    DXGI_OUTDUPL_FRAME_INFO, IDXGIOutput, IDXGIOutput5, IDXGIOutputDuplication, IDXGIResource,
};
use windows::core::{Error, Interface, Result};

use super::copy_texture;
use super::scrgb::Converter;
use super::white_level::{DeviceName, WhiteLevels};

const FIRST_FRAME_MILLISECONDS: u32 = 500;
const REARM_MILLISECONDS: u32 = 50;

pub struct Duplicator {
    output: IDXGIOutput5,
    device: ID3D11Device,
    name: DeviceName,
    duplication: Option<IDXGIOutputDuplication>,
    held: Option<ID3D11Texture2D>,
    holding: bool,
    has_image: bool,
    target_holds_image: bool,
    pointer: PointerReport,
    pixels: Pixels,
    converter: Option<Converter>,
    refresh_milliseconds: u32,
}

// Until this duplication reports the pointer apart, the image may already hold it.
#[derive(Default)]
struct PointerReport {
    apart: bool,
}

impl Duplicator {
    pub fn open(output: &IDXGIOutput, device: &ID3D11Device) -> Result<Self> {
        let mut duplicator = Self {
            output: output.cast()?,
            device: device.clone(),
            name: unsafe { output.GetDesc() }?.DeviceName,
            duplication: None,
            held: None,
            holding: false,
            has_image: false,
            target_holds_image: false,
            pointer: PointerReport::default(),
            pixels: Pixels::Bgra8,
            converter: None,
            refresh_milliseconds: refresh_period(0, 0),
        };
        duplicator.reopen()?;
        Ok(duplicator)
    }

    pub fn begin(
        &mut self,
        context: &ID3D11DeviceContext,
        target: &ID3D11Texture2D,
        levels: &mut WhiteLevels,
    ) -> Result<()> {
        if self.has_image && self.held.is_some() {
            self.present(context, target, levels)?;
        }
        self.release()
    }

    // Every monitor's frame arrives while the first is waited for, so the waits overlap, not add up.
    pub fn finish(
        &mut self,
        begun: Result<()>,
        begun_at: Instant,
        context: &ID3D11DeviceContext,
        target: &ID3D11Texture2D,
        levels: &mut WhiteLevels,
    ) -> Result<(bool, Pixels)> {
        let waited = u32::try_from(begun_at.elapsed().as_millis()).unwrap_or(u32::MAX);
        let timeout = self.refresh_milliseconds.saturating_sub(waited);
        match begun.and_then(|()| self.acquire(timeout)) {
            Ok(true) => self
                .present(context, target, levels)
                .map(|pixels| (true, pixels)),
            Ok(false) if self.has_image && self.target_holds_image => Ok((false, self.pixels)),
            Ok(false) => Err(no_image()),
            Err(error) if is_lost(&error) => {
                self.reopen()?;
                self.present(context, target, levels)
                    .map(|pixels| (true, pixels))
            }
            Err(error) => Err(error),
        }
    }

    pub fn rearm(&mut self) {
        if !self.holding && self.acquire(REARM_MILLISECONDS).is_err() {
            self.reopen_quietly();
        }
    }

    // Not held, against Microsoft's advice: a frame held until the next tick makes that tick miss its update.
    pub fn next(
        &mut self,
        context: &ID3D11DeviceContext,
        target: Option<&ID3D11Texture2D>,
        levels: &mut WhiteLevels,
        draw: impl FnOnce(&ID3D11Texture2D, Pixels) -> Result<()>,
    ) -> Result<bool> {
        if let Some(target) = target
            && self.has_image
            && self.held.is_some()
            && !self.target_holds_image
        {
            self.present(context, target, levels)?;
        }
        self.release()?;
        let drawn = self.draw_fresh(context, target, levels, draw);
        let released = self.release();
        drawn.and_then(|drawn| released.map(|()| drawn))
    }

    fn draw_fresh(
        &mut self,
        context: &ID3D11DeviceContext,
        target: Option<&ID3D11Texture2D>,
        levels: &mut WhiteLevels,
        draw: impl FnOnce(&ID3D11Texture2D, Pixels) -> Result<()>,
    ) -> Result<bool> {
        if !self.acquire(0)? {
            return Ok(false);
        }
        let Some(frame) = self.held.clone() else {
            return Ok(false);
        };
        let pixels = match (target, format(&frame)) {
            (Some(target), _) => self.present(context, target, levels)?,
            (None, DXGI_FORMAT_B8G8R8A8_UNORM) => Pixels::Bgra8,
            (None, DXGI_FORMAT_R16G16B16A16_FLOAT) => levels.pixels(&self.name)?,
            (None, other) => {
                return Err(Error::new(
                    E_FAIL,
                    format!("the desktop image came in format {}", other.0),
                ));
            }
        };
        draw(&frame, pixels).map(|()| true)
    }

    pub fn refresh(
        &mut self,
        context: &ID3D11DeviceContext,
        target: &ID3D11Texture2D,
        levels: &mut WhiteLevels,
    ) -> Result<bool> {
        self.release()?;
        let fresh = match self.acquire(0) {
            Ok(fresh) => fresh,
            Err(error) if is_lost(&error) => {
                self.reopen_quietly();
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        if fresh {
            self.present(context, target, levels)?;
        }
        self.release().map(|()| fresh)
    }

    // Opened again, a duplication was seen to bring no image while the desktop stayed still, so it is the last resort.
    pub fn settle(&mut self) {
        self.hold(Instant::now());
        if !(self.has_image && (self.holding || self.target_holds_image)) {
            self.reopen_quietly();
        }
    }

    // While no frame is held, Windows copies every desktop update into the duplication.
    pub fn hold(&mut self, since: Instant) {
        if !self.holding {
            let waited = u32::try_from(since.elapsed().as_millis()).unwrap_or(u32::MAX);
            let _ = self.acquire(self.refresh_milliseconds.saturating_sub(waited));
        }
    }

    pub fn reopen_quietly(&mut self) {
        let _ = self.reopen();
    }

    pub fn forget_target(&mut self) {
        self.target_holds_image = false;
    }

    pub fn pointer_apart(&self) -> bool {
        self.pointer.apart
    }

    fn present(
        &mut self,
        context: &ID3D11DeviceContext,
        target: &ID3D11Texture2D,
        levels: &mut WhiteLevels,
    ) -> Result<Pixels> {
        let frame = match (&self.held, self.has_image) {
            (Some(held), true) => held.clone(),
            _ => return Err(no_image()),
        };
        self.pixels = match format(&frame) {
            DXGI_FORMAT_B8G8R8A8_UNORM => {
                copy_texture(context, target, &frame)?;
                Pixels::Bgra8
            }
            DXGI_FORMAT_R16G16B16A16_FLOAT => {
                let pixels = levels.pixels(&self.name)?;
                self.converter()?
                    .convert(context, target, &frame, pixels.scale())?;
                pixels
            }
            other => {
                return Err(Error::new(
                    E_FAIL,
                    format!("the desktop image came in format {}", other.0),
                ));
            }
        };
        self.target_holds_image = true;
        Ok(self.pixels)
    }

    fn converter(&mut self) -> Result<&Converter> {
        let converter = match self.converter.take() {
            Some(converter) => converter,
            None => Converter::new(&self.device)?,
        };
        Ok(self.converter.insert(converter))
    }

    fn reopen(&mut self) -> Result<()> {
        self.held = None;
        self.holding = false;
        self.has_image = false;
        self.pointer = PointerReport::default();
        self.duplication = None;
        // A driver refusing the float format must not cost the monitor its duplication.
        let duplicate =
            |formats: &[_]| unsafe { self.output.DuplicateOutput1(&self.device, 0, formats) };
        let duplication = duplicate(&[DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_B8G8R8A8_UNORM])
            .or_else(|_| duplicate(&[DXGI_FORMAT_B8G8R8A8_UNORM]))?;
        let description = unsafe { duplication.GetDesc() };
        let rate = description.ModeDesc.RefreshRate;
        self.refresh_milliseconds = refresh_period(rate.Numerator, rate.Denominator);
        self.duplication = Some(duplication);
        self.acquire(FIRST_FRAME_MILLISECONDS)?;
        let linear = self
            .held
            .as_ref()
            .is_some_and(|held| format(held) == DXGI_FORMAT_R16G16B16A16_FLOAT);
        if linear {
            self.converter()?;
        }
        Ok(())
    }

    fn acquire(&mut self, timeout: u32) -> Result<bool> {
        let Some(duplication) = &self.duplication else {
            return Err(DXGI_ERROR_INVALID_CALL.into());
        };
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        match unsafe { duplication.AcquireNextFrame(timeout, &mut info, &mut resource) } {
            Ok(()) => {
                self.holding = true;
                let resource = resource.ok_or_else(|| Error::from(E_POINTER))?;
                self.held = Some(resource.cast()?);
                self.pointer.update(&info);
                let fresh = info.LastPresentTime != 0;
                self.has_image |= fresh;
                self.target_holds_image &= !fresh;
                Ok(fresh)
            }
            Err(error) if error.code() == DXGI_ERROR_WAIT_TIMEOUT => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn release(&mut self) -> Result<()> {
        self.held = None;
        if std::mem::take(&mut self.holding)
            && let Some(duplication) = &self.duplication
        {
            unsafe { duplication.ReleaseFrame() }?;
        }
        Ok(())
    }
}

fn format(texture: &ID3D11Texture2D) -> DXGI_FORMAT {
    let mut description = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut description) };
    description.Format
}

impl PointerReport {
    // The pointer is reported only with a mouse update, and as not visible when the image already holds it.
    fn update(&mut self, info: &DXGI_OUTDUPL_FRAME_INFO) {
        if info.LastMouseUpdateTime != 0 {
            self.apart = info.PointerPosition.Visible.as_bool();
        }
    }
}

fn no_image() -> Error {
    Error::new(E_POINTER, "the duplication has no desktop image yet")
}

fn is_lost(error: &Error) -> bool {
    error.code() == DXGI_ERROR_ACCESS_LOST || error.code() == DXGI_ERROR_INVALID_CALL
}

fn refresh_period(numerator: u32, denominator: u32) -> u32 {
    if numerator == 0 || denominator == 0 {
        return 17;
    }
    let milliseconds = (u64::from(denominator) * 1000).div_ceil(u64::from(numerator));
    u32::try_from(milliseconds).unwrap_or(u32::MAX).clamp(4, 34)
}

#[cfg(test)]
mod tests {
    use windows::Win32::Graphics::Dxgi::{DXGI_OUTDUPL_FRAME_INFO, DXGI_OUTDUPL_POINTER_POSITION};

    use super::{PointerReport, refresh_period};

    fn frame(present: i64, mouse_update: i64, visible: bool) -> DXGI_OUTDUPL_FRAME_INFO {
        DXGI_OUTDUPL_FRAME_INFO {
            LastPresentTime: present,
            LastMouseUpdateTime: mouse_update,
            PointerPosition: DXGI_OUTDUPL_POINTER_POSITION {
                Visible: visible.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn a_duplication_holds_the_pointer_in_its_image_until_it_reports_it_apart() {
        let mut report = PointerReport::default();
        assert!(!report.apart);
        report.update(&frame(5, 0, true));
        assert!(!report.apart);
        report.update(&frame(0, 6, true));
        assert!(report.apart);
        report.update(&frame(7, 0, false));
        assert!(report.apart);
        report.update(&frame(8, 8, false));
        assert!(!report.apart);
        report.update(&frame(0, 9, true));
        assert!(report.apart);
    }

    #[test]
    fn a_reopened_duplication_forgets_the_last_report() {
        let mut report = PointerReport::default();
        report.update(&frame(1, 1, true));
        assert!(report.apart);
        report = PointerReport::default();
        report.update(&frame(2, 0, true));
        assert!(!report.apart);
    }

    #[test]
    fn refresh_period_rounds_up_and_stays_in_range() {
        assert_eq!(refresh_period(60, 1), 17);
        assert_eq!(refresh_period(59_940, 1_000), 17);
        assert_eq!(refresh_period(144, 1), 7);
        assert_eq!(refresh_period(500, 1), 4);
        assert_eq!(refresh_period(24, 1), 34);
        assert_eq!(refresh_period(0, 1), 17);
        assert_eq!(refresh_period(60, 0), 17);
    }
}
