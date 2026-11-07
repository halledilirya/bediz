mod paint;

use bediz_core::geometry::{Point, Rect};
use windows::Win32::Foundation::{HINSTANCE, HWND, RECT};
use windows::Win32::Graphics::Direct2D::{ID2D1Bitmap1, ID2D1Image};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, IDWriteFontCollection,
    IDWriteTextFormat,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAK, DWMWA_TRANSITIONS_FORCEDISABLED, DWMWINDOWATTRIBUTE, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_MWA_NO_ALT_ENTER, DXGI_MWA_NO_WINDOW_CHANGES, DXGI_PRESENT, DXGI_SCALING_NONE,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_DISCARD,
    DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGISwapChain1,
};
use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromRect};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, HWND_TOPMOST, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOSIZE, SetWindowPos,
    ShowWindow, USER_DEFAULT_SCREEN_DPI, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{BOOL, Result, w};

use super::gpu::Gpu;
use super::readback;
use super::window_class::{OVERLAY_CLASS, OwnedWindow};
use paint::Painter;

const PARKED: i32 = -32000;
const LABEL_FONT_SIZE: f32 = 12.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub selection: Option<Rect>,
    pub highlight: Option<Rect>,
    pub labelled: bool,
    pub pointer: Option<Point>,
}

struct Surfaces {
    target: ID2D1Bitmap1,
    image: ID3D11Texture2D,
    image_bitmap: ID2D1Bitmap1,
}

pub struct Overlay {
    surfaces: Option<Surfaces>,
    swap_chain: IDXGISwapChain1,
    window: OwnedWindow,
    rect: Rect,
    dpi: u32,
    text: IDWriteTextFormat,
}

impl Overlay {
    pub fn create(instance: HINSTANCE, gpu: &Gpu, rect: Rect, allocate: bool) -> Result<Self> {
        let (width, height) = (rect.width(), rect.height());
        let window = OwnedWindow(unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                OVERLAY_CLASS,
                w!("Bediz"),
                WS_POPUP,
                PARKED,
                PARKED,
                i32::try_from(width).unwrap_or(i32::MAX),
                i32::try_from(height).unwrap_or(i32::MAX),
                None,
                None,
                Some(instance),
                None,
            )
        }?);
        set_flag(window.0, DWMWA_TRANSITIONS_FORCEDISABLED, true)?;
        set_flag(window.0, DWMWA_CLOAK, true)?;
        let _ = unsafe { ShowWindow(window.0, SW_SHOWNOACTIVATE) };
        let description = DXGI_SWAP_CHAIN_DESC1 {
            Width: 1,
            Height: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_NONE,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            ..Default::default()
        };
        let swap_chain = unsafe {
            gpu.factory
                .CreateSwapChainForHwnd(&gpu.device, window.0, &description, None, None)
        }?;
        unsafe {
            gpu.factory
                .MakeWindowAssociation(window.0, DXGI_MWA_NO_ALT_ENTER | DXGI_MWA_NO_WINDOW_CHANGES)
        }?;
        let dpi = monitor_dpi(rect);
        let mut overlay = Self {
            surfaces: None,
            swap_chain,
            window,
            rect,
            dpi,
            text: text_format(gpu, scale_of(dpi))?,
        };
        if allocate {
            overlay.allocate(gpu)?;
        }
        Ok(overlay)
    }

    pub fn window(&self) -> HWND {
        self.window.0
    }

    pub fn rect(&self) -> Rect {
        self.rect
    }

    pub fn scale(&self) -> f32 {
        scale_of(self.dpi)
    }

    pub fn refresh_scale(&mut self, gpu: &Gpu) -> Result<()> {
        let dpi = monitor_dpi(self.rect);
        if dpi != self.dpi {
            self.text = text_format(gpu, scale_of(dpi))?;
            self.dpi = dpi;
        }
        Ok(())
    }

    pub fn image(&self) -> Option<&ID3D11Texture2D> {
        self.surfaces.as_ref().map(|surfaces| &surfaces.image)
    }

    pub fn allocate(&mut self, gpu: &Gpu) -> Result<()> {
        if self.surfaces.is_some() {
            return Ok(());
        }
        let (width, height) = (self.rect.width(), self.rect.height());
        self.resize(width, height)?;
        let target = gpu.target_bitmap(&self.swap_chain)?;
        let image = gpu.image_texture(width, height)?;
        let image_bitmap = gpu.source_bitmap(&image)?;
        self.surfaces = Some(Surfaces {
            target,
            image,
            image_bitmap,
        });
        Ok(())
    }

    pub fn release(&mut self) -> Result<()> {
        if self.surfaces.take().is_none() {
            return Ok(());
        }
        self.resize(1, 1)
    }

    pub fn pixel(&self, gpu: &Gpu, at: Point) -> Option<[u8; 4]> {
        let surfaces = self.surfaces.as_ref()?;
        if !self.rect.contains(at) {
            return None;
        }
        let local = Point::new(at.x - self.rect.left, at.y - self.rect.top);
        readback::read_pixel(gpu, &surfaces.image, local).ok()
    }

    pub fn draw(&self, gpu: &Gpu, view: &View) -> Result<()> {
        let Some(surfaces) = &self.surfaces else {
            return Ok(());
        };
        let (left, top) = (self.rect.left, self.rect.top);
        let bounds = self.rect.translated(-left, -top);
        let local = |area: Rect| area.translated(-left, -top);
        let framed = view
            .selection
            .map(|area| (local(area), true))
            .or(view.highlight.map(|area| (local(area), false)));
        let pointer = view
            .pointer
            .filter(|pointer| self.rect.contains(*pointer))
            .map(|pointer| Point::new(pointer.x - left, pointer.y - top));
        let colour = view.pointer.and_then(|pointer| self.pixel(gpu, pointer));
        let painter = Painter {
            gpu,
            text: &self.text,
            scale: self.scale(),
            bounds,
        };
        unsafe {
            gpu.canvas.SetTarget(&surfaces.target);
            gpu.canvas.BeginDraw();
        }
        painter.image(&surfaces.image_bitmap);
        painter.dim(framed.and_then(|(area, _)| area.intersection(&bounds)));
        if let Some((area, with_handles)) = framed {
            painter.frame(area, with_handles);
            if view.labelled {
                painter.size_label(area);
            }
        }
        if let Some(at) = pointer {
            painter.magnifier(&surfaces.image_bitmap, at, colour);
        }
        let drawn = unsafe { gpu.canvas.EndDraw(None, None) };
        unsafe { gpu.canvas.SetTarget(None::<&ID2D1Image>) };
        drawn?;
        unsafe { self.swap_chain.Present(1, DXGI_PRESENT(0)) }.ok()
    }

    pub fn place(&self) -> Result<()> {
        unsafe {
            SetWindowPos(
                self.window.0,
                Some(HWND_TOPMOST),
                self.rect.left,
                self.rect.top,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE,
            )
        }
    }

    pub fn uncloak(&self) -> Result<()> {
        set_flag(self.window.0, DWMWA_CLOAK, false)
    }

    pub fn hide(&self) -> Result<()> {
        let cloaked = set_flag(self.window.0, DWMWA_CLOAK, true);
        let parked = unsafe {
            SetWindowPos(
                self.window.0,
                Some(HWND_TOPMOST),
                PARKED,
                PARKED,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE,
            )
        };
        cloaked.and(parked)
    }

    fn resize(&self, width: u32, height: u32) -> Result<()> {
        unsafe {
            self.swap_chain.ResizeBuffers(
                0,
                width,
                height,
                DXGI_FORMAT_UNKNOWN,
                DXGI_SWAP_CHAIN_FLAG(0),
            )
        }
    }
}

fn monitor_dpi(rect: Rect) -> u32 {
    let bounds = RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    };
    let monitor = unsafe { MonitorFromRect(&bounds, MONITOR_DEFAULTTONEAREST) };
    let (mut dpi_x, mut dpi_y) = (0, 0);
    match unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) } {
        Ok(()) if dpi_x > 0 => dpi_x,
        _ => USER_DEFAULT_SCREEN_DPI,
    }
}

fn scale_of(dpi: u32) -> f32 {
    dpi as f32 / USER_DEFAULT_SCREEN_DPI as f32
}

fn text_format(gpu: &Gpu, scale: f32) -> Result<IDWriteTextFormat> {
    let format = unsafe {
        gpu.text.CreateTextFormat(
            w!("Segoe UI"),
            None::<&IDWriteFontCollection>,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            LABEL_FONT_SIZE * scale,
            w!(""),
        )
    }?;
    unsafe {
        format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
    }
    Ok(format)
}

fn set_flag(window: HWND, attribute: DWMWINDOWATTRIBUTE, value: bool) -> Result<()> {
    let value = BOOL::from(value);
    unsafe {
        DwmSetWindowAttribute(
            window,
            attribute,
            (&value as *const BOOL).cast(),
            size_of::<BOOL>() as u32,
        )
    }
}
