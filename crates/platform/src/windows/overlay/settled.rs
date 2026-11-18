//! The marks before the first one being drawn, held, selected or typed, laid once over the image
//! under the selection and kept, so a redraw draws only that mark and the ones after it.

use bediz_core::geometry::Rect;
use bediz_core::markup::Mark;
use windows::Win32::Graphics::Direct2D::{ID2D1Bitmap1, ID2D1Image};
use windows::core::Result;

use super::paint::Painter;
use crate::windows::marking::Decor;

pub struct Layer {
    pub bitmap: ID2D1Bitmap1,
    pub marks: usize,
}

#[derive(Clone, Debug, PartialEq)]
struct Drawn {
    mark: Mark,
    preview: Option<u64>,
}

#[derive(Default)]
pub struct Settled {
    bitmap: Option<ID2D1Bitmap1>,
    area: Option<Rect>,
    drawn: Vec<Drawn>,
}

impl Settled {
    pub fn lay(
        &mut self,
        painter: &Painter,
        image: &ID2D1Bitmap1,
        area: Rect,
        decor: &Decor,
        previews: &mut Vec<(u64, ID2D1Bitmap1)>,
    ) -> Option<Layer> {
        let marks = decor.marks.get(..decor.settled).unwrap_or(decor.marks);
        if marks.is_empty() {
            return None;
        }
        let stamps: Vec<Option<u64>> = marks
            .iter()
            .map(|mark| decor.preview_of(mark).map(|preview| preview.stamp))
            .collect();
        let from = kept(&self.drawn, self.area, area, marks, &stamps);
        let Ok(bitmap) = self.bitmap(painter) else {
            self.forget();
            return None;
        };
        if from < marks.len() {
            let canvas = &painter.gpu.canvas;
            unsafe {
                canvas.SetTarget(&bitmap);
                canvas.BeginDraw();
            }
            if from == 0 {
                painter.image(image, area);
            }
            painter.marks(decor, marks.get(from..).unwrap_or_default(), area, previews);
            let ended = unsafe { canvas.EndDraw(None, None) };
            unsafe { canvas.SetTarget(None::<&ID2D1Image>) };
            if ended.is_err() {
                self.forget();
                return None;
            }
        }
        self.drawn.truncate(from);
        self.drawn.extend(
            marks
                .iter()
                .zip(stamps)
                .skip(from)
                .map(|(mark, preview)| Drawn {
                    mark: mark.clone(),
                    preview,
                }),
        );
        self.area = Some(area);
        Some(Layer {
            bitmap,
            marks: marks.len(),
        })
    }

    pub fn forget(&mut self) {
        *self = Self::default();
    }

    fn bitmap(&mut self, painter: &Painter) -> Result<ID2D1Bitmap1> {
        if let Some(bitmap) = &self.bitmap {
            return Ok(bitmap.clone());
        }
        let bounds = painter.bounds;
        let made = painter.gpu.layer_bitmap(bounds.width(), bounds.height())?;
        self.bitmap = Some(made.clone());
        Ok(made)
    }
}

fn kept(
    drawn: &[Drawn],
    over: Option<Rect>,
    area: Rect,
    marks: &[Mark],
    stamps: &[Option<u64>],
) -> usize {
    let unchanged = over == Some(area)
        && drawn.len() <= marks.len()
        && drawn
            .iter()
            .zip(marks.iter().zip(stamps))
            .all(|(drawn, (mark, stamp))| drawn.mark == *mark && drawn.preview == *stamp);
    if unchanged { drawn.len() } else { 0 }
}

#[cfg(test)]
mod tests {
    use bediz_core::geometry::Point;
    use bediz_core::image::Image;
    use bediz_core::markup::{Colour, Markup, Shape, Swatch, Tool};
    use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
    use windows::core::Interface;

    use super::*;
    use crate::windows::gpu::tests::warp;
    use crate::windows::readback;

    const WIDTH: u32 = 160;
    const HEIGHT: u32 = 120;
    const RED: Colour = Swatch::Red.colour();

    fn line(id: u64, x: i32) -> Mark {
        Mark {
            id,
            shape: Shape::Line(Point::new(x, 0), Point::new(x, 9)),
            colour: RED,
            width: 3.0,
        }
    }

    fn drawn(marks: &[Mark]) -> Vec<Drawn> {
        marks
            .iter()
            .map(|mark| Drawn {
                mark: mark.clone(),
                preview: None,
            })
            .collect()
    }

    #[test]
    fn the_marks_laid_stay_while_only_the_ones_after_them_change() {
        let area = Rect::new(0, 0, 100, 100);
        let marks = [line(1, 10), line(2, 20), line(3, 30)];
        let none = [None; 3];
        let laid = drawn(&marks[..2]);
        assert_eq!(kept(&laid, Some(area), area, &marks, &none), 2);
        assert_eq!(kept(&laid, Some(area), area, &marks[..2], &none[..2]), 2);
        assert_eq!(kept(&[], None, area, &marks, &none), 0);
    }

    #[test]
    fn the_layer_is_laid_again_when_a_laid_mark_its_blur_or_the_selection_changes() {
        let area = Rect::new(0, 0, 100, 100);
        let marks = [line(1, 10), line(2, 20)];
        let none = [None; 2];
        let laid = drawn(&marks);
        let moved = [line(1, 15), line(2, 20)];
        assert_eq!(kept(&laid, Some(area), area, &moved, &none), 0);
        assert_eq!(kept(&laid, Some(area), area, &marks, &[None, Some(4)]), 0);
        let smaller = Rect::new(0, 0, 100, 90);
        assert_eq!(kept(&laid, Some(area), smaller, &marks, &none), 0);
        assert_eq!(kept(&laid, Some(area), area, &marks[..1], &none[..1]), 0);
    }

    fn ground() -> Image {
        let bgra = (0..WIDTH * HEIGHT)
            .flat_map(|index| {
                let (x, y) = (index % WIDTH, index / WIDTH);
                [
                    (x * 255 / WIDTH) as u8,
                    (y * 255 / HEIGHT) as u8,
                    (x ^ y) as u8,
                    255,
                ]
            })
            .collect();
        Image::from_bgra(WIDTH, HEIGHT, bgra).unwrap()
    }

    fn frame(
        painter: &Painter,
        image: &ID2D1Bitmap1,
        area: Rect,
        markup: &Markup,
        layer: Option<&Layer>,
    ) -> Vec<u8> {
        let gpu = painter.gpu;
        let decor = Decor {
            marks: markup.marks(),
            settled: markup.settled(),
            ..Decor::default()
        };
        let target = gpu.layer_bitmap(WIDTH, HEIGHT).unwrap();
        unsafe {
            gpu.canvas.SetTarget(&target);
            gpu.canvas.BeginDraw();
        }
        painter.marked(image, area, &decor, layer, &mut Vec::new());
        unsafe { gpu.canvas.EndDraw(None, None) }.unwrap();
        unsafe { gpu.canvas.SetTarget(None::<&ID2D1Image>) };
        let texture: ID3D11Texture2D = unsafe { target.GetSurface() }.unwrap().cast().unwrap();
        let whole = Rect::new(0, 0, WIDTH as i32, HEIGHT as i32);
        readback::read(gpu, &texture, whole)
            .unwrap()
            .bgra()
            .to_vec()
    }

    fn stroke(markup: &mut Markup, tool: Tool, points: &[(i32, i32)]) {
        let mut points = points.iter().map(|&(x, y)| Point::new(x, y));
        let (Some(first), Some(last)) = (points.next(), points.next_back()) else {
            panic!("a stroke has two ends");
        };
        markup.draw(tool, first, RED, 4.0);
        points.for_each(|at| markup.drag(at, false));
        assert!(markup.finish(last, false));
    }

    fn laid_as_drawn(
        settled: &mut Settled,
        painter: &Painter,
        image: &ID2D1Bitmap1,
        area: Rect,
        markup: &Markup,
    ) {
        let decor = Decor {
            marks: markup.marks(),
            settled: markup.settled(),
            ..Decor::default()
        };
        let layer = settled.lay(painter, image, area, &decor, &mut Vec::new());
        assert_eq!(
            layer.as_ref().map(|layer| layer.marks),
            Some(markup.settled())
        );
        let direct = frame(painter, image, area, markup, None);
        let laid = frame(painter, image, area, markup, layer.as_ref());
        let differing = direct
            .chunks(4)
            .zip(laid.chunks(4))
            .position(|(direct, laid)| direct[..3] != laid[..3]);
        assert_eq!(differing, None, "the first pixel that differs");
    }

    #[test]
    fn marks_laid_once_show_the_same_pixels_as_marks_drawn_each_time() {
        let gpu = warp();
        let texture = gpu.texture_with(&ground()).unwrap();
        let image = gpu.source_bitmap(&texture).unwrap();
        let text = super::super::text_format(&gpu, 1.0).unwrap();
        let painter = Painter {
            gpu: &gpu,
            text: &text,
            scale: 1.0,
            bounds: Rect::new(0, 0, WIDTH as i32, HEIGHT as i32),
            origin: Point::new(0, 0),
        };
        let mut markup = Markup::default();
        stroke(
            &mut markup,
            Tool::Pen,
            &[(12, 14), (40, 60), (75, 30), (110, 90)],
        );
        stroke(
            &mut markup,
            Tool::Highlighter,
            &[(5, 70), (60, 40), (150, 50)],
        );
        stroke(&mut markup, Tool::Rectangle, &[(30, 20), (130, 100)]);
        markup.draw(Tool::Text, Point::new(20, 80), Swatch::Blue.colour(), 4.0);
        markup.type_text("Ab");
        markup.end_editing();
        stroke(&mut markup, Tool::Redact, &[(120, 5), (170, 40)]);
        markup.draw(Tool::Arrow, Point::new(10, 110), RED, 4.0);
        markup.drag(Point::new(90, 60), false);
        let area = Rect::new(8, 6, 152, 114);
        let mut settled = Settled::default();
        laid_as_drawn(&mut settled, &painter, &image, area, &markup);
        assert!(markup.finish(Point::new(100, 55), false));
        stroke(&mut markup, Tool::Pen, &[(140, 110), (100, 70), (60, 105)]);
        markup.draw(Tool::Ellipse, Point::new(50, 50), RED, 4.0);
        markup.drag(Point::new(80, 75), false);
        laid_as_drawn(&mut settled, &painter, &image, area, &markup);
        assert_eq!(settled.drawn.len(), markup.marks().len() - 1);
        markup.abandon();
        assert!(markup.grab(Point::new(30, 60), 3, area));
        markup.drag(Point::new(45, 64), false);
        laid_as_drawn(&mut settled, &painter, &image, area, &markup);
        laid_as_drawn(
            &mut settled,
            &painter,
            &image,
            Rect::new(0, 0, 100, 120),
            &markup,
        );
    }
}
