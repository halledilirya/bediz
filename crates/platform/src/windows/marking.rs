//! Marking up a screenshot's selection on the overlay: the chosen tool, colour and width, the
//! measured size of each text, the blurs and pixelations made from what lies under them, and
//! the image copied or saved with every mark on it.

pub mod layout;
pub mod paint;
mod render;

use bediz_core::geometry::{Point, Rect};
use bediz_core::image::Image;
use bediz_core::markup::effect::{self, Effect, Job};
use bediz_core::markup::typing::Cursor;
use bediz_core::markup::{DEFAULT_WIDTH, Mark, Markup, Swatch, THICKEST, THINNEST, Tool, shape};
use windows::Win32::Foundation::{E_FAIL, HWND, POINT};
use windows::Win32::UI::Input::Ime::{
    CFS_POINT, COMPOSITIONFORM, ImmGetContext, ImmReleaseContext, ImmSetCompositionWindow,
};
use windows::core::{Error, Result};

use super::scene::Scene;

#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    pub stamp: u64,
    pub effect: Effect,
    width: u32,
    beneath: Vec<Mark>,
    under: Vec<u64>,
}

#[derive(Clone, Copy, Default)]
pub struct Decor<'a> {
    pub marks: &'a [Mark],
    pub previews: &'a [Preview],
    pub selected: Option<&'a Mark>,
    pub editing: Option<&'a Mark>,
    pub cursor: Cursor,
}

impl Decor<'_> {
    pub fn preview_of(&self, mark: &Mark) -> Option<&Preview> {
        let area = effect::area_of(mark)?;
        self.previews.iter().find(|preview| {
            preview.effect.id == mark.id
                && preview.effect.area == area
                && preview.width == width_key(mark.width)
        })
    }
}

pub struct Marking {
    markup: Markup,
    tool: Option<Tool>,
    swatch: Swatch,
    width: f32,
    step: f32,
    previews: Vec<Preview>,
    refreshed: Option<u64>,
    stamps: u64,
    high_surrogate: Option<u16>,
}

impl Marking {
    pub fn new(scale: f32) -> Self {
        let scale = scale.max(1.0);
        Self {
            markup: Markup::default(),
            tool: None,
            swatch: Swatch::default(),
            width: (DEFAULT_WIDTH * scale).round(),
            step: scale.round(),
            previews: Vec::new(),
            refreshed: None,
            stamps: 0,
            high_surrogate: None,
        }
    }

    pub fn markup(&self) -> &Markup {
        &self.markup
    }

    pub fn markup_mut(&mut self) -> &mut Markup {
        &mut self.markup
    }

    pub fn tool(&self) -> Option<Tool> {
        self.tool
    }

    pub fn swatch(&self) -> Swatch {
        self.swatch
    }

    pub fn version(&self) -> u64 {
        self.markup
            .revision()
            .wrapping_mul(31)
            .wrapping_add(self.stamps)
            .wrapping_add(self.tool.map_or(0, |tool| tool as u64 + 1) << 48)
    }

    pub fn decor(&self) -> Decor<'_> {
        Decor {
            marks: self.markup.marks(),
            previews: &self.previews,
            selected: self.markup.selected(),
            editing: self.markup.editing(),
            cursor: self.markup.cursor().unwrap_or_default(),
        }
    }

    pub fn measure_texts(&mut self, scene: &Scene) {
        if self.markup.is_empty() {
            return;
        }
        let Ok(gpu) = scene.gpu_near(Point::default()) else {
            return;
        };
        self.markup
            .measure_texts(|text| layout::measure(&gpu.text, text));
    }

    pub fn choose(&mut self, tool: Tool) {
        self.markup.end_editing();
        self.markup.deselect();
        self.tool = if self.tool == Some(tool) {
            None
        } else {
            Some(tool)
        };
    }

    pub fn recolour(&mut self, swatch: Swatch) {
        self.swatch = swatch;
        self.markup.recolour(swatch.colour());
    }

    pub fn widen(&mut self, steps: i32) {
        let base = self
            .markup
            .editing()
            .or(self.markup.selected())
            .map_or(self.width, |mark| mark.width);
        let width = (base + steps as f32 * self.step).clamp(THINNEST, THICKEST);
        self.width = width;
        self.markup.rewidth(width);
    }

    pub fn draw(&mut self, tool: Tool, at: Point) {
        self.markup.draw(tool, at, self.swatch.colour(), self.width);
    }

    pub fn type_unit(&mut self, unit: u16) {
        if (0xD800..0xDC00).contains(&unit) {
            self.high_surrogate = Some(unit);
            return;
        }
        let units: Vec<u16> = match self.high_surrogate.take() {
            Some(high) if (0xDC00..0xE000).contains(&unit) => vec![high, unit],
            _ => vec![unit],
        };
        if let Ok(typed) = String::from_utf16(&units) {
            self.markup.type_text(&typed);
        }
    }

    pub fn refresh_previews(&mut self, scene: &Scene) {
        if !self.stale() {
            return;
        }
        self.previews = made(self.markup.marks(), &self.previews, scene, &mut self.stamps);
        self.refreshed = Some(self.markup.revision());
    }

    fn stale(&self) -> bool {
        !self.markup.is_busy() && self.refreshed != Some(self.markup.revision())
    }

    pub fn compose(&self, image: Image, rect: Rect, scene: &Scene) -> Result<Image> {
        let marks = self.markup.marks();
        if marks.is_empty() {
            return Ok(image);
        }
        let mut stamps = self.stamps;
        let previews = made(marks, &self.previews, scene, &mut stamps);
        let missing = effect::jobs(marks).any(|job| {
            job.area.intersection(&rect).is_some()
                && !previews
                    .iter()
                    .any(|preview| preview.effect.id == job.mark.id)
        });
        if missing {
            return Err(Error::new(E_FAIL, "a blur or pixelation could not be made"));
        }
        let effects: Vec<&Effect> = previews.iter().map(|preview| &preview.effect).collect();
        let gpu = scene.gpu_near(Point::new(rect.left, rect.top))?;
        render::layered(image, rect, marks, &effects, gpu)
    }

    pub fn place_composition(&self, window: HWND, origin: Point) {
        let Some(mark) = self.markup.editing() else {
            return;
        };
        let bounds = shape::bounds(mark);
        let context = unsafe { ImmGetContext(window) };
        if context.is_invalid() {
            return;
        }
        let form = COMPOSITIONFORM {
            dwStyle: CFS_POINT,
            ptCurrentPos: POINT {
                x: bounds.left - origin.x,
                y: bounds.bottom - origin.y,
            },
            ..Default::default()
        };
        unsafe {
            let _ = ImmSetCompositionWindow(context, &form);
            let _ = ImmReleaseContext(window, context);
        }
    }
}

fn made(marks: &[Mark], earlier: &[Preview], scene: &Scene, stamps: &mut u64) -> Vec<Preview> {
    let mut made: Vec<Preview> = Vec::new();
    for job in effect::jobs(marks) {
        let under: Vec<u64> = made.iter().map(|preview| preview.stamp).collect();
        if let Some(kept) = reusable(earlier, &job, &under) {
            made.push(kept.clone());
            continue;
        }
        let Some(effect) = beneath(&job, &made, scene)
            .ok()
            .and_then(|source| effect::apply(job.mark, &source))
        else {
            continue;
        };
        *stamps = stamps.wrapping_add(1);
        made.push(Preview {
            stamp: *stamps,
            effect,
            width: width_key(job.mark.width),
            beneath: job.beneath.to_vec(),
            under,
        });
    }
    made
}

// The captured image stays as it was, so the same marks beneath make the same pixels.
fn reusable<'a>(earlier: &'a [Preview], job: &Job, under: &[u64]) -> Option<&'a Preview> {
    earlier.iter().find(|preview| {
        preview.effect.id == job.mark.id
            && preview.effect.area == job.area
            && preview.width == width_key(job.mark.width)
            && preview.beneath.as_slice() == job.beneath
            && preview.under.as_slice() == under
    })
}

fn beneath(job: &Job, made: &[Preview], scene: &Scene) -> Result<Image> {
    let screen = scene.read(job.area)?;
    if job.beneath.is_empty() {
        return Ok(screen);
    }
    let gpu = scene.gpu_near(Point::new(job.area.left, job.area.top))?;
    let effects: Vec<&Effect> = made.iter().map(|preview| &preview.effect).collect();
    render::layered(screen, job.area, job.beneath, &effects, gpu)
}

fn width_key(width: f32) -> u32 {
    (width * 16.0).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use bediz_core::markup::Shape;

    fn made_over(job: &Job, under: Vec<u64>) -> Preview {
        Preview {
            stamp: 1,
            effect: Effect {
                id: job.mark.id,
                area: job.area,
                image: Image::from_bgra(1, 1, vec![0, 0, 0, 255]).unwrap(),
            },
            width: width_key(job.mark.width),
            beneath: job.beneath.to_vec(),
            under,
        }
    }

    #[test]
    fn a_blur_is_made_again_only_when_what_lies_beneath_it_changes() {
        let mut marking = Marking::new(1.0);
        marking.draw(Tool::Pen, Point::new(0, 0));
        marking.markup_mut().finish(Point::new(30, 30), false);
        marking.draw(Tool::Blur, Point::new(10, 10));
        marking.markup_mut().finish(Point::new(60, 60), false);
        marking.draw(Tool::Arrow, Point::new(0, 50));
        marking.markup_mut().finish(Point::new(90, 50), false);
        let marks = marking.markup().marks().to_vec();
        let Some(job) = effect::jobs(&marks).next() else {
            panic!("the blur is a job");
        };
        let earlier = [made_over(&job, Vec::new())];
        assert!(reusable(&earlier, &job, &[]).is_some());
        assert!(reusable(&earlier, &job, &[7]).is_none());
        let mut moved = marking.markup().clone();
        assert!(moved.grab(Point::new(3, 3), 2, Rect::new(0, 0, 100, 100)));
        moved.finish(Point::new(13, 3), false);
        assert!(matches!(
            moved.selected().map(|mark| &mark.shape),
            Some(Shape::Pen(_))
        ));
        let Some(job) = effect::jobs(moved.marks()).next() else {
            panic!("the blur is still a job");
        };
        assert!(reusable(&earlier, &job, &[]).is_none());
    }

    #[test]
    fn the_wheel_grows_the_selected_mark_from_its_own_width() {
        let mut marking = Marking::new(1.0);
        marking.draw(Tool::Line, Point::new(0, 0));
        marking.markup_mut().finish(Point::new(40, 0), false);
        assert!(marking.markup_mut().rewidth(10.0));
        marking.widen(1);
        assert_eq!(
            marking.markup().selected().map(|mark| mark.width),
            Some(11.0)
        );
        assert_eq!(marking.width, 11.0);
    }

    #[test]
    fn choosing_another_tool_lets_go_of_the_mark_just_drawn() {
        let mut marking = Marking::new(1.0);
        marking.choose(Tool::Rectangle);
        marking.draw(Tool::Rectangle, Point::new(0, 0));
        marking.markup_mut().finish(Point::new(40, 30), false);
        assert!(marking.markup().selected().is_some());
        marking.choose(Tool::Arrow);
        assert!(marking.markup().selected().is_none());
    }

    #[test]
    fn choosing_the_tool_in_use_puts_it_down() {
        let mut marking = Marking::new(1.0);
        marking.choose(Tool::Arrow);
        assert_eq!(marking.tool(), Some(Tool::Arrow));
        marking.choose(Tool::Pen);
        assert_eq!(marking.tool(), Some(Tool::Pen));
        marking.choose(Tool::Pen);
        assert_eq!(marking.tool(), None);
    }

    #[test]
    fn the_width_follows_the_display_scale_and_stays_in_bounds() {
        let mut marking = Marking::new(2.0);
        assert_eq!(marking.width, DEFAULT_WIDTH * 2.0);
        marking.widen(1);
        assert_eq!(marking.width, DEFAULT_WIDTH * 2.0 + 2.0);
        marking.widen(-100);
        assert_eq!(marking.width, THINNEST);
        marking.widen(100);
        assert_eq!(marking.width, THICKEST);
    }

    #[test]
    fn a_character_typed_in_two_halves_arrives_whole_and_a_lone_half_is_dropped() {
        let mut marking = Marking::new(1.0);
        marking.draw(Tool::Text, Point::new(0, 0));
        for unit in "a😀".encode_utf16() {
            marking.type_unit(unit);
        }
        marking.type_unit(0xD83D);
        marking.type_unit(u16::from(b'b'));
        marking.type_unit(0xDE00);
        let text = marking.markup().editing().map(|mark| mark.shape.clone());
        assert_eq!(
            text,
            Some(Shape::Text {
                at: Point::new(0, 0),
                text: "a😀b".to_owned(),
                extent: None,
            })
        );
    }

    #[test]
    fn previews_are_made_again_after_each_change_but_not_during_a_drag() {
        let mut marking = Marking::new(1.0);
        marking.draw(Tool::Text, Point::new(0, 0));
        marking.refreshed = Some(marking.markup().revision());
        assert!(!marking.stale());
        marking.type_unit(u16::from(b'a'));
        assert!(marking.stale());
        marking.draw(Tool::Blur, Point::new(0, 0));
        marking.markup_mut().drag(Point::new(20, 20), false);
        assert!(!marking.stale());
        marking.markup_mut().finish(Point::new(40, 40), false);
        assert!(marking.stale());
        marking.refreshed = Some(marking.markup().revision());
        marking.choose(Tool::Arrow);
        assert!(marking.stale());
    }

    #[test]
    fn a_new_colour_is_kept_for_the_next_mark() {
        let mut marking = Marking::new(1.0);
        marking.recolour(Swatch::Blue);
        marking.draw(Tool::Line, Point::new(0, 0));
        marking.markup_mut().finish(Point::new(10, 0), false);
        assert_eq!(
            marking.markup().marks().first().map(|mark| mark.colour),
            Some(Swatch::Blue.colour())
        );
    }
}
