use std::mem::ManuallyDrop;

use windows::Win32::Foundation::{E_POINTER, HMODULE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_NONE,
    D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_CAP_STYLE_FLAT,
    D2D1_DASH_STYLE_CUSTOM, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_LINE_JOIN_MITER,
    D2D1_STROKE_STYLE_PROPERTIES, D2D1_UNIT_MODE_PIXELS, ID2D1Bitmap1, ID2D1DeviceContext,
    ID2D1Factory, ID2D1Factory1, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWriteCreateFactory, IDWriteFactory,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    IDXGIAdapter1, IDXGIDevice, IDXGIFactory2, IDXGISurface, IDXGISwapChain1,
};
use windows::core::{Error, Interface, Result};

use super::contrast::Colours;

const DIM: D2D1_COLOR_F = D2D1_COLOR_F {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.45,
};
const BORDER: D2D1_COLOR_F = D2D1_COLOR_F {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};
const OUTLINE: D2D1_COLOR_F = D2D1_COLOR_F {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.8,
};
const GRID: D2D1_COLOR_F = D2D1_COLOR_F {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.2,
};
const DASHES: [f32; 2] = [4.0, 4.0];

pub struct Gpu {
    pub adapter: IDXGIAdapter1,
    pub factory: IDXGIFactory2,
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    pub canvas: ID2D1DeviceContext,
    pub text: IDWriteFactory,
    pub dim: ID2D1SolidColorBrush,
    pub border: ID2D1SolidColorBrush,
    pub outline: ID2D1SolidColorBrush,
    pub grid: ID2D1SolidColorBrush,
    pub dashed: ID2D1StrokeStyle,
    pub pixel: ID3D11Texture2D,
}

impl Gpu {
    pub fn create(
        factory: &IDXGIFactory2,
        adapter: &IDXGIAdapter1,
        d2d: &ID2D1Factory1,
    ) -> Result<Self> {
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
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
        let context: ID3D11DeviceContext = context.ok_or_else(|| Error::from(E_POINTER))?;
        let dxgi_device: IDXGIDevice = device.cast()?;
        let d2d_device = unsafe { d2d.CreateDevice(&dxgi_device) }?;
        let canvas = unsafe { d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE) }?;
        unsafe { canvas.SetUnitMode(D2D1_UNIT_MODE_PIXELS) };
        let dim = unsafe { canvas.CreateSolidColorBrush(&DIM, None) }?;
        let border = unsafe { canvas.CreateSolidColorBrush(&BORDER, None) }?;
        let outline = unsafe { canvas.CreateSolidColorBrush(&OUTLINE, None) }?;
        let grid = unsafe { canvas.CreateSolidColorBrush(&GRID, None) }?;
        let dashed = unsafe {
            ID2D1Factory::CreateStrokeStyle(
                d2d,
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_FLAT,
                    endCap: D2D1_CAP_STYLE_FLAT,
                    dashCap: D2D1_CAP_STYLE_FLAT,
                    lineJoin: D2D1_LINE_JOIN_MITER,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_CUSTOM,
                    dashOffset: 0.0,
                },
                Some(&DASHES),
            )
        }?;
        let text = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }?;
        let pixel = texture(&device, &staging_description(1, 1))?;
        Ok(Self {
            adapter: adapter.clone(),
            factory: factory.clone(),
            device,
            context,
            canvas,
            text,
            dim,
            border,
            outline,
            grid,
            dashed,
            pixel,
        })
    }

    pub fn use_colours(&self, colours: Option<&Colours>) {
        let (ink, ground) =
            colours.map_or((BORDER, OUTLINE), |colours| (colours.ink, colours.ground));
        unsafe {
            self.border.SetColor(&ink);
            self.outline.SetColor(&ground);
        }
    }

    // D3D11 frees released resources only at the next flush, and Direct2D keeps its own caches.
    pub fn free_released(&self) -> Result<()> {
        let device = unsafe { self.canvas.GetDevice() }?;
        unsafe {
            device.ClearResources(0);
            self.context.Flush();
        }
        Ok(())
    }

    pub fn image_texture(&self, width: u32, height: u32) -> Result<ID3D11Texture2D> {
        image_texture(&self.device, width, height)
    }

    pub fn staging_texture(&self, width: u32, height: u32) -> Result<ID3D11Texture2D> {
        texture(&self.device, &staging_description(width, height))
    }

    pub fn target_bitmap(&self, swap_chain: &IDXGISwapChain1) -> Result<ID2D1Bitmap1> {
        let surface: IDXGISurface = unsafe { swap_chain.GetBuffer(0) }?;
        self.bitmap(
            &surface,
            D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
        )
    }

    pub fn source_bitmap(&self, texture: &ID3D11Texture2D) -> Result<ID2D1Bitmap1> {
        let surface: IDXGISurface = texture.cast()?;
        self.bitmap(&surface, D2D1_BITMAP_OPTIONS_NONE)
    }

    fn bitmap(&self, surface: &IDXGISurface, options: D2D1_BITMAP_OPTIONS) -> Result<ID2D1Bitmap1> {
        let properties = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: options,
            colorContext: ManuallyDrop::new(None),
        };
        unsafe {
            self.canvas
                .CreateBitmapFromDxgiSurface(surface, Some(&properties))
        }
    }
}

pub fn image_texture(device: &ID3D11Device, width: u32, height: u32) -> Result<ID3D11Texture2D> {
    texture(
        device,
        &D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET).0 as u32,
            ..texture_description(width, height)
        },
    )
}

fn texture(device: &ID3D11Device, description: &D3D11_TEXTURE2D_DESC) -> Result<ID3D11Texture2D> {
    let mut texture = None;
    unsafe { device.CreateTexture2D(description, None, Some(&mut texture)) }?;
    texture.ok_or_else(|| Error::from(E_POINTER))
}

fn staging_description(width: u32, height: u32) -> D3D11_TEXTURE2D_DESC {
    D3D11_TEXTURE2D_DESC {
        Usage: D3D11_USAGE_STAGING,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        ..texture_description(width, height)
    }
}

fn texture_description(width: u32, height: u32) -> D3D11_TEXTURE2D_DESC {
    D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        ..Default::default()
    }
}
