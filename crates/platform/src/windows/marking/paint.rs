//! Draws marks with Direct2D, the same way on the overlay and in the copied or saved image:
//! strokes, lines and arrows, boxes and ellipses, texts with their colour emoji, numbered steps,
//! black boxes and the images of blurs and pixelations.

use bediz_core::geometry::{Point, Rect};
use bediz_core::markup::shape::{self, Spot};
use bediz_core::markup::{Colour, Mark, Shape};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW,
    D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID,
    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_ELLIPSE,
    D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_LINE_JOIN_ROUND, D2D1_STROKE_STYLE_PROPERTIES,
    ID2D1Bitmap1, ID2D1DeviceContext, ID2D1PathGeometry, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD, DWRITE_MEASURING_MODE_NATURAL, IDWriteFactory,
    IDWriteTextFormat,
};
use windows::core::Result;
use windows_numerics::Vector2;

use super::layout;

const HIGHLIGHTER_OPACITY: f32 = 0.4;
const STEP_TEXT: f32 = 1.1;

pub struct Ink<'a> {
    pub canvas: &'a ID2D1DeviceContext,
    pub text: &'a IDWriteFactory,
    pub dx: f32,
    pub dy: f32,
}

impl Ink<'_> {
    pub fn mark(&self, mark: &Mark) -> Result<()> {
        let round = self.round()?;
        match &mark.shape {
            Shape::Pen(points) => self.stroke(points, mark.colour, 1.0, mark.width, &round),
            Shape::Highlighter(points) => self.stroke(
                points,
                mark.colour,
                HIGHLIGHTER_OPACITY,
                shape::highlighter_width(mark.width),
                &round,
            ),
            Shape::Line(from, to) => {
                let brush = self.brush(mark.colour, 1.0)?;
                unsafe {
                    self.canvas.DrawLine(
                        point(self.at(Spot::of(*from))),
                        point(self.at(Spot::of(*to))),
                        &brush,
                        mark.width,
                        &round,
                    );
                }
                Ok(())
            }
            Shape::Arrow(from, to) => self.arrow(*from, *to, mark, &round),
            Shape::Rectangle(from, to) => {
                let brush = self.brush(mark.colour, 1.0)?;
                let area = self.boxed(*from, *to);
                unsafe {
                    self.canvas.DrawRectangle(&area, &brush, mark.width, &round);
                }
                Ok(())
            }
            Shape::Ellipse(from, to) => {
                let brush = self.brush(mark.colour, 1.0)?;
                let oval = ellipse(self.boxed(*from, *to));
                unsafe {
                    self.canvas.DrawEllipse(&oval, &brush, mark.width, &round);
                }
                Ok(())
            }
            Shape::Text { at, text, .. } => self.text(*at, text, mark),
            Shape::Step { at, number } => self.step(*at, *number, mark),
            Shape::Redact(..) | Shape::Blur(..) | Shape::Pixelate(..) => Ok(()),
        }
    }

    pub fn cover(&self, area: Rect) -> Result<()> {
        let black = self.brush(Colour::new(0, 0, 0), 1.0)?;
        unsafe {
            let previous = self.canvas.GetAntialiasMode();
            // Aliased, so no pixel at its edge is left partly see-through.
            self.canvas.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
            self.canvas.FillRectangle(&self.rect(area), &black);
            self.canvas.SetAntialiasMode(previous);
        }
        Ok(())
    }

    pub fn picture(&self, bitmap: &ID2D1Bitmap1, area: Rect) {
        unsafe {
            self.canvas.DrawBitmap(
                bitmap,
                Some(&self.rect(area)),
                1.0,
                D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                None,
                None,
            );
        }
    }

    pub fn brush(&self, colour: Colour, alpha: f32) -> Result<ID2D1SolidColorBrush> {
        let colour = D2D1_COLOR_F {
            r: f32::from(colour.red) / 255.0,
            g: f32::from(colour.green) / 255.0,
            b: f32::from(colour.blue) / 255.0,
            a: alpha,
        };
        unsafe { self.canvas.CreateSolidColorBrush(&colour, None) }
    }

    pub fn boxed(&self, from: Point, to: Point) -> D2D_RECT_F {
        let (a, b) = (self.at(Spot::of(from)), self.at(Spot::of(to)));
        D2D_RECT_F {
            left: a.x.min(b.x),
            top: a.y.min(b.y),
            right: a.x.max(b.x),
            bottom: a.y.max(b.y),
        }
    }

    fn stroke(
        &self,
        points: &[Point],
        colour: Colour,
        alpha: f32,
        width: f32,
        round: &ID2D1StrokeStyle,
    ) -> Result<()> {
        let brush = self.brush(colour, alpha)?;
        let spots: Vec<Spot> = points.iter().map(|at| self.at(Spot::of(*at))).collect();
        let Some((first, rest)) = spots.split_first() else {
            return Ok(());
        };
        if rest.is_empty() {
            let dot = D2D1_ELLIPSE {
                point: point(*first),
                radiusX: width / 2.0,
                radiusY: width / 2.0,
            };
            unsafe { self.canvas.FillEllipse(&dot, &brush) };
            return Ok(());
        }
        let path = self.path(*first, rest, false)?;
        unsafe { self.canvas.DrawGeometry(&path, &brush, width, round) };
        Ok(())
    }

    fn arrow(&self, from: Point, to: Point, mark: &Mark, round: &ID2D1StrokeStyle) -> Result<()> {
        let Some(head) = shape::arrow_head(from, to, mark.width) else {
            return Ok(());
        };
        let brush = self.brush(mark.colour, 1.0)?;
        unsafe {
            self.canvas.DrawLine(
                point(self.at(Spot::of(from))),
                point(self.at(head.shaft_end)),
                &brush,
                mark.width,
                round,
            );
        }
        let corners = [self.at(head.left), self.at(head.right)];
        let triangle = self.path(self.at(head.tip), &corners, true)?;
        unsafe { self.canvas.FillGeometry(&triangle, &brush, None) };
        Ok(())
    }

    fn text(&self, at: Point, text: &str, mark: &Mark) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        let laid = layout::text_layout(self.text, text, shape::text_size(mark.width))?;
        let brush = self.brush(mark.colour, 1.0)?;
        unsafe {
            self.canvas.DrawTextLayout(
                point(self.origin(at)),
                &laid,
                &brush,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            );
        }
        Ok(())
    }

    fn step(&self, at: Point, number: u32, mark: &Mark) -> Result<()> {
        let radius = shape::step_radius(mark.width);
        let centre = self.at(Spot::of(at));
        let disc = D2D1_ELLIPSE {
            point: point(centre),
            radiusX: radius,
            radiusY: radius,
        };
        let fill = self.brush(mark.colour, 1.0)?;
        let ink = self.brush(mark.colour.ink(), 1.0)?;
        unsafe { self.canvas.FillEllipse(&disc, &fill) };
        let format = self.format(radius * STEP_TEXT, DWRITE_FONT_WEIGHT_BOLD, true)?;
        let wide: Vec<u16> = number.to_string().encode_utf16().collect();
        let area = D2D_RECT_F {
            left: centre.x - radius,
            top: centre.y - radius,
            right: centre.x + radius,
            bottom: centre.y + radius,
        };
        unsafe {
            self.canvas.DrawText(
                &wide,
                &format,
                &area,
                &ink,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
        Ok(())
    }

    fn path(&self, first: Spot, rest: &[Spot], closed: bool) -> Result<ID2D1PathGeometry> {
        let factory = unsafe { self.canvas.GetFactory() }?;
        let path = unsafe { factory.CreatePathGeometry() }?;
        let sink = unsafe { path.Open() }?;
        let lines: Vec<Vector2> = rest.iter().map(|spot| point(*spot)).collect();
        let (begin, end) = if closed {
            (D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED)
        } else {
            (D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN)
        };
        unsafe {
            sink.BeginFigure(point(first), begin);
            sink.AddLines(&lines);
            sink.EndFigure(end);
            sink.Close()?;
        }
        Ok(path)
    }

    fn round(&self) -> Result<ID2D1StrokeStyle> {
        let factory = unsafe { self.canvas.GetFactory() }?;
        unsafe {
            factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_SOLID,
                    dashOffset: 0.0,
                },
                None,
            )
        }
    }

    fn format(
        &self,
        size: f32,
        weight: DWRITE_FONT_WEIGHT,
        centred: bool,
    ) -> Result<IDWriteTextFormat> {
        layout::text_format(self.text, size, weight, centred)
    }

    fn origin(&self, at: Point) -> Spot {
        self.at(Spot {
            x: at.x as f32,
            y: at.y as f32,
        })
    }

    fn at(&self, spot: Spot) -> Spot {
        Spot {
            x: spot.x - self.dx,
            y: spot.y - self.dy,
        }
    }

    fn rect(&self, area: Rect) -> D2D_RECT_F {
        D2D_RECT_F {
            left: area.left as f32 - self.dx,
            top: area.top as f32 - self.dy,
            right: area.right as f32 - self.dx,
            bottom: area.bottom as f32 - self.dy,
        }
    }
}

pub fn ellipse(area: D2D_RECT_F) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE {
        point: Vector2::new(
            (area.left + area.right) / 2.0,
            (area.top + area.bottom) / 2.0,
        ),
        radiusX: (area.right - area.left) / 2.0,
        radiusY: (area.bottom - area.top) / 2.0,
    }
}

fn point(spot: Spot) -> Vector2 {
    Vector2::new(spot.x, spot.y)
}
