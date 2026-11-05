use windows::Win32::Foundation::E_POINTER;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_INVALID_CALL, DXGI_ERROR_WAIT_TIMEOUT,
    DXGI_OUTDUPL_FRAME_INFO, IDXGIOutput, IDXGIOutput5, IDXGIOutputDuplication, IDXGIResource,
};
use windows::core::{Error, Interface, Result};

use super::copy_texture;

const FIRST_FRAME_MILLISECONDS: u32 = 500;
const REARM_MILLISECONDS: u32 = 50;

pub struct Duplicator {
    output: IDXGIOutput5,
    device: ID3D11Device,
    duplication: Option<IDXGIOutputDuplication>,
    held: Option<ID3D11Texture2D>,
    holding: bool,
    has_image: bool,
    refresh_milliseconds: u32,
}

impl Duplicator {
    pub fn open(output: &IDXGIOutput, device: &ID3D11Device) -> Result<Self> {
        let mut duplicator = Self {
            output: output.cast()?,
            device: device.clone(),
            duplication: None,
            held: None,
            holding: false,
            has_image: false,
            refresh_milliseconds: refresh_period(0, 0),
        };
        duplicator.reopen()?;
        Ok(duplicator)
    }

    pub fn begin(&mut self, context: &ID3D11DeviceContext, target: &ID3D11Texture2D) -> Result<()> {
        if self.has_image
            && let Some(held) = &self.held
        {
            copy_texture(context, target, held)?;
        }
        self.release()
    }

    pub fn finish(
        &mut self,
        begun: Result<()>,
        context: &ID3D11DeviceContext,
        target: &ID3D11Texture2D,
    ) -> Result<bool> {
        match begun.and_then(|()| self.acquire(self.refresh_milliseconds)) {
            Ok(true) => self.copy_held(context, target).map(|()| true),
            Ok(false) if self.has_image => Ok(false),
            Ok(false) => Err(no_image()),
            Err(error) if is_lost(&error) => {
                self.reopen()?;
                self.copy_held(context, target)?;
                Ok(true)
            }
            Err(error) => Err(error),
        }
    }

    pub fn rearm(&mut self) {
        if !self.holding && self.acquire(REARM_MILLISECONDS).is_err() {
            self.reopen_quietly();
        }
    }

    pub fn reopen_quietly(&mut self) {
        let _ = self.reopen();
    }

    fn copy_held(&self, context: &ID3D11DeviceContext, target: &ID3D11Texture2D) -> Result<()> {
        match (&self.held, self.has_image) {
            (Some(held), true) => copy_texture(context, target, held),
            _ => Err(no_image()),
        }
    }

    fn reopen(&mut self) -> Result<()> {
        self.held = None;
        self.holding = false;
        self.has_image = false;
        self.duplication = None;
        let duplication = unsafe {
            self.output
                .DuplicateOutput1(&self.device, 0, &[DXGI_FORMAT_B8G8R8A8_UNORM])
        }?;
        let description = unsafe { duplication.GetDesc() };
        let rate = description.ModeDesc.RefreshRate;
        self.refresh_milliseconds = refresh_period(rate.Numerator, rate.Denominator);
        self.duplication = Some(duplication);
        self.acquire(FIRST_FRAME_MILLISECONDS)?;
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
                let fresh = info.LastPresentTime != 0;
                self.has_image |= fresh;
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
    use super::refresh_period;

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
