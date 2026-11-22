//! The devices every pin is drawn with, one Direct3D and one DirectComposition device shared by
//! all of them, apart from the overlay's so a display change or an idle release leaves pins be.

use windows::Win32::Foundation::{E_POINTER, HMODULE};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice2, IDCompositionDesktopDevice, IDCompositionDevice,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO, IDXGIAdapter, IDXGIAdapter3,
    IDXGIDevice,
};
use windows::core::{Error, Interface, Result};

pub struct Devices {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    pub composition: IDCompositionDesktopDevice,
    state: Option<IDCompositionDevice>,
    adapter: Option<IDXGIAdapter3>,
}

impl Devices {
    pub fn create() -> Result<Self> {
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None::<&IDXGIAdapter>,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }?;
        let device: ID3D11Device = device.ok_or_else(|| Error::from(E_POINTER))?;
        let context = context.ok_or_else(|| Error::from(E_POINTER))?;
        let dxgi: IDXGIDevice = device.cast()?;
        let adapter = unsafe { dxgi.GetAdapter() }
            .and_then(|adapter| adapter.cast())
            .ok();
        let composition: IDCompositionDesktopDevice = unsafe { DCompositionCreateDevice2(&dxgi) }?;
        let state = composition.cast().ok();
        Ok(Self {
            device,
            context,
            composition,
            state,
            adapter,
        })
    }

    // Windows gives each process a share of graphics memory; a pin past it is not made at all.
    pub fn has_room(&self, bytes: u64) -> bool {
        let Some(adapter) = &self.adapter else {
            return true;
        };
        let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        match unsafe { adapter.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info) }
        {
            Ok(()) => info.CurrentUsage.saturating_add(bytes) <= info.Budget,
            Err(_) => true,
        }
    }

    // Only a clear answer counts as lost, so a check that fails cannot remake the devices on end.
    pub fn is_lost(&self) -> bool {
        let invalid = self
            .state
            .as_ref()
            .and_then(|state| unsafe { state.CheckDeviceState() }.ok())
            .is_some_and(|valid| !valid.as_bool());
        invalid || unsafe { self.device.GetDeviceRemovedReason() }.is_err()
    }

    pub fn commit(&self) -> Result<()> {
        unsafe { self.composition.Commit() }
    }
}
