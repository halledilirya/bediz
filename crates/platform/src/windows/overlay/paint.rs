use bediz_core::geometry::{Point, Rect};
use bediz_core::image::hex_colour;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_ROUNDED_RECT,
    ID2D1Bitmap1, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_TEXT_METRICS, IDWriteTextFormat,
};

use crate::windows::gpu::Gpu;

const HANDLE_SIZE: f32 = 7.0;
const LABEL_GAP: f32 = 6.0;
const LABEL_PADDING_X: f32 = 6.0;
const LABEL_PADDING_Y: f32 = 3.0;
const LABEL_RADIUS: f32 = 3.0;
const MAGNIFIER_CELLS: i32 = 15;
const MAGNIFIER_CELL: f32 = 8.0;
const MAGNIFIER_OFFSET: f32 = 20.0;

pub struct Painter<'a> {
    pub gpu: &'a Gpu,
    pub text: &'a IDWriteTextFormat,
    pub scale: f32,
    pub bounds: Rect,
}

impl Painter<'_> {
    pub fn image(&self, bitmap: &ID2D1Bitmap1) {
        unsafe {
            self.gpu.canvas.DrawBitmap(
                bitmap,
                Some(&rect_f(self.bounds)),
                1.0,
                D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                None,
                None,
            );
        }
    }

    pub fn dim(&self, selection: Option<Rect>) {
        let full = rect_f(self.bounds);
        let canvas = &self.gpu.canvas;
        match selection {
            None => unsafe { canvas.FillRectangle(&full, &self.gpu.dim) },
            Some(area) => {
                for shade in shades(&full, &rect_f(area)) {
                    unsafe { canvas.FillRectangle(&shade, &self.gpu.dim) };
                }
            }
        }
    }

    pub fn frame(&self, area: Rect, with_handles: bool) {
        let edge = D2D_RECT_F {
            left: area.left as f32 + 0.5,
            top: area.top as f32 + 0.5,
            right: area.right as f32 - 0.5,
            bottom: area.bottom as f32 - 0.5,
        };
        let gpu = self.gpu;
        unsafe {
            gpu.canvas
                .DrawRectangle(&edge, &gpu.outline, 1.0, None::<&ID2D1StrokeStyle>);
            gpu.canvas
                .DrawRectangle(&edge, &gpu.border, 1.0, &gpu.dashed);
        }
        if !with_handles {
            return;
        }
        for handle in handles(&edge, (HANDLE_SIZE * self.scale).round()) {
            unsafe {
                gpu.canvas.FillRectangle(&handle, &gpu.border);
                gpu.canvas
                    .DrawRectangle(&handle, &gpu.outline, 1.0, None::<&ID2D1StrokeStyle>);
            }
        }
    }

    pub fn size_label(&self, area: Rect) {
        let text = format!("{} \u{00d7} {}", area.width(), area.height());
        let Some((width, height)) = self.measure(&text) else {
            return;
        };
        let full = rect_f(self.bounds);
        let gap = LABEL_GAP * self.scale;
        let above = area.top as f32 - gap - height;
        let top = if above < full.top {
            area.top as f32 + gap
        } else {
            above
        };
        let left = (area.left as f32).min(full.right - width).max(full.left);
        self.label(
            &text,
            D2D_RECT_F {
                left,
                top,
                right: left + width,
                bottom: top + height,
            },
        );
    }

    pub fn magnifier(&self, bitmap: &ID2D1Bitmap1, at: Point, colour: Option<[u8; 4]>) {
        let full = rect_f(self.bounds);
        let cell = (MAGNIFIER_CELL * self.scale).round();
        let size = cell * MAGNIFIER_CELLS as f32;
        let offset = MAGNIFIER_OFFSET * self.scale;
        let label = colour
            .map(hex_colour)
            .and_then(|text| self.measure(&text).map(|(_, height)| (text, height)));
        let label_height = label.as_ref().map_or(0.0, |(_, height)| *height);
        let (x, y) = (at.x as f32, at.y as f32);
        let left = if x + offset + size > full.right {
            x - offset - size
        } else {
            x + offset
        };
        let top = if y + offset + size + label_height > full.bottom {
            y - offset - size - label_height
        } else {
            y + offset
        };
        let view = D2D_RECT_F {
            left,
            top,
            right: left + size,
            bottom: top + size,
        };
        let gpu = self.gpu;
        unsafe {
            gpu.canvas.FillRectangle(
                &D2D_RECT_F {
                    left: left - 1.0,
                    top: top - 1.0,
                    right: view.right + 1.0,
                    bottom: view.bottom + 1.0,
                },
                &gpu.outline,
            );
        }
        let radius = MAGNIFIER_CELLS / 2;
        let wanted = Rect::new(
            at.x - radius,
            at.y - radius,
            at.x + radius + 1,
            at.y + radius + 1,
        );
        if let Some(visible) = wanted.intersection(&self.bounds) {
            let destination = D2D_RECT_F {
                left: left + (visible.left - wanted.left) as f32 * cell,
                top: top + (visible.top - wanted.top) as f32 * cell,
                right: left + (visible.right - wanted.left) as f32 * cell,
                bottom: top + (visible.bottom - wanted.top) as f32 * cell,
            };
            unsafe {
                gpu.canvas.DrawBitmap(
                    bitmap,
                    Some(&destination),
                    1.0,
                    D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                    Some(&rect_f(visible)),
                    None,
                );
            }
        }
        for line in 1..MAGNIFIER_CELLS {
            let at = line as f32 * cell;
            let vertical = D2D_RECT_F {
                left: left + at,
                top,
                right: left + at + 1.0,
                bottom: view.bottom,
            };
            let horizontal = D2D_RECT_F {
                left,
                top: top + at,
                right: view.right,
                bottom: top + at + 1.0,
            };
            unsafe {
                gpu.canvas.FillRectangle(&vertical, &gpu.grid);
                gpu.canvas.FillRectangle(&horizontal, &gpu.grid);
            }
        }
        let middle = radius as f32 * cell;
        let center = D2D_RECT_F {
            left: left + middle + 0.5,
            top: top + middle + 0.5,
            right: left + middle + cell + 0.5,
            bottom: top + middle + cell + 0.5,
        };
        unsafe {
            gpu.canvas
                .DrawRectangle(&center, &gpu.outline, 2.0, None::<&ID2D1StrokeStyle>);
            gpu.canvas
                .DrawRectangle(&center, &gpu.border, 1.0, None::<&ID2D1StrokeStyle>);
        }
        if let Some((text, height)) = label {
            self.label(
                &text,
                D2D_RECT_F {
                    left,
                    top: view.bottom + 1.0,
                    right: view.right,
                    bottom: view.bottom + 1.0 + height,
                },
            );
        }
    }

    fn measure(&self, text: &str) -> Option<(f32, f32)> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let layout = unsafe {
            self.gpu
                .text
                .CreateTextLayout(&wide, self.text, f32::MAX, f32::MAX)
        }
        .ok()?;
        let mut metrics = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metrics) }.ok()?;
        Some((
            (metrics.width + 2.0 * LABEL_PADDING_X * self.scale).ceil(),
            (metrics.height + 2.0 * LABEL_PADDING_Y * self.scale).ceil(),
        ))
    }

    fn label(&self, text: &str, rect: D2D_RECT_F) {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let radius = LABEL_RADIUS * self.scale;
        let background = D2D1_ROUNDED_RECT {
            rect,
            radiusX: radius,
            radiusY: radius,
        };
        let gpu = self.gpu;
        unsafe {
            gpu.canvas.FillRoundedRectangle(&background, &gpu.outline);
            gpu.canvas.DrawText(
                &wide,
                self.text,
                &rect,
                &gpu.border,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }
}

fn rect_f(rect: Rect) -> D2D_RECT_F {
    D2D_RECT_F {
        left: rect.left as f32,
        top: rect.top as f32,
        right: rect.right as f32,
        bottom: rect.bottom as f32,
    }
}

fn shades(full: &D2D_RECT_F, hole: &D2D_RECT_F) -> [D2D_RECT_F; 4] {
    [
        D2D_RECT_F {
            bottom: hole.top,
            ..*full
        },
        D2D_RECT_F {
            top: hole.bottom,
            ..*full
        },
        D2D_RECT_F {
            left: full.left,
            top: hole.top,
            right: hole.left,
            bottom: hole.bottom,
        },
        D2D_RECT_F {
            left: hole.right,
            top: hole.top,
            right: full.right,
            bottom: hole.bottom,
        },
    ]
}

fn handles(edge: &D2D_RECT_F, size: f32) -> [D2D_RECT_F; 8] {
    let half = size / 2.0;
    let middle_x = ((edge.left + edge.right) / 2.0).round() + 0.5;
    let middle_y = ((edge.top + edge.bottom) / 2.0).round() + 0.5;
    let square = |x: f32, y: f32| D2D_RECT_F {
        left: x - half,
        top: y - half,
        right: x + half,
        bottom: y + half,
    };
    [
        square(edge.left, edge.top),
        square(middle_x, edge.top),
        square(edge.right, edge.top),
        square(edge.right, middle_y),
        square(edge.right, edge.bottom),
        square(middle_x, edge.bottom),
        square(edge.left, edge.bottom),
        square(edge.left, middle_y),
    ]
}
