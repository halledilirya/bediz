use std::collections::VecDeque;
use std::time::Duration;

use bediz_core::image::Image;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, ID3D11DeviceContext,
    ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::DXGI_ERROR_WAS_STILL_DRAWING;
use windows::core::Result;

use super::pointer::Sample;
use crate::windows::gpu::Gpu;
use crate::windows::readback;

const SLOTS: usize = 3;

pub struct Ring {
    slots: Vec<ID3D11Texture2D>,
    pending: VecDeque<(usize, Duration, Option<Sample>)>,
    next: usize,
    width: u32,
    height: u32,
}

impl Ring {
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Result<Self> {
        let slots = (0..SLOTS)
            .map(|_| gpu.staging_texture(width, height))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            slots,
            pending: VecDeque::with_capacity(SLOTS),
            next: 0,
            width,
            height,
        })
    }

    // A slot still waiting to be read is overwritten, so the oldest frame gives way, never the capture.
    pub fn submit(
        &mut self,
        context: &ID3D11DeviceContext,
        source: &ID3D11Texture2D,
        at: Duration,
        pointer: Option<Sample>,
    ) {
        let slot = self.next;
        let Some(texture) = self.slots.get(slot) else {
            return;
        };
        self.pending.retain(|(waiting, _, _)| *waiting != slot);
        unsafe { context.CopyResource(texture, source) };
        self.pending.push_back((slot, at, pointer));
        self.next = (slot + 1) % SLOTS;
    }

    pub fn discard(&mut self) {
        self.pending.clear();
    }

    pub fn collect(
        &mut self,
        context: &ID3D11DeviceContext,
        wait: bool,
    ) -> Result<Vec<(Image, Duration, Option<Sample>)>> {
        let flags = if wait {
            0
        } else {
            D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32
        };
        let mut ready = Vec::new();
        while let Some(&(slot, at, _)) = self.pending.front() {
            let Some(texture) = self.slots.get(slot) else {
                self.pending.pop_front();
                continue;
            };
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            match unsafe { context.Map(texture, 0, D3D11_MAP_READ, flags, Some(&mut mapped)) } {
                Ok(()) => {
                    let image = readback::mapped_image(&mapped, self.width, self.height);
                    unsafe { context.Unmap(texture, 0) };
                    let pointer = self.pending.pop_front().and_then(|(_, _, pointer)| pointer);
                    ready.push((image?, at, pointer));
                }
                Err(error) if error.code() == DXGI_ERROR_WAS_STILL_DRAWING => break,
                Err(error) => return Err(error),
            }
        }
        Ok(ready)
    }
}
