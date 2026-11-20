//! Marking up a screenshot's selection on the overlay: the chosen tool, colour and width, the
//! measured size of each text, the blurs and pixelations made from what lies under them, and
//! the image copied or saved with every mark on it.

mod composing;
pub mod layout;
mod maker;
pub mod paint;
mod render;

use bediz_core::geometry::{Point, Rect};
use bediz_core::image::Image;
use bediz_core::markup::composition::{self, Composed, Composition};
use bediz_core::markup::effect::{self, Effect, Job};
use bediz_core::markup::typing::Cursor;
use bediz_core::markup::{DEFAULT_WIDTH, Mark, Markup, Swatch, THICKEST, THINNEST, Tool};
use bediz_core::ocr::Turn;
use windows::Win32::Foundation::{E_FAIL, HWND};
use windows::core::{Error, Result};

use super::scene::Scene;
use maker::{Maker, Order};

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
    pub settled: usize,
    pub previews: &'a [Preview],
    pub selected: Option<&'a Mark>,
    pub editing: Option<&'a Mark>,
    pub cursor: Cursor,
    pub composing: Option<&'a Composition>,
    pub words: &'a [Rect],
    pub words_turn: Option<Turn>,
}

impl Decor<'_> {
    pub fn composed(&self) -> Option<Composed> {
        composition::composed(self.editing?, self.cursor, self.composing?)
    }

    pub fn preview_of(&self, mark: &Mark) -> Option<&Preview> {
        let area = effect::area_of(mark)?;
        self.previews
            .iter()
            .find(|preview| preview.effect.id == mark.id && preview.effect.area == area)
    }
}

struct Asked {
    width: u32,
    beneath: Vec<Mark>,
    under: Vec<u64>,
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
    maker: Maker,
    asked: Option<Asked>,
    high_surrogate: Option<u16>,
    within: Option<Rect>,
    composing: bool,
}

impl Marking {
    pub fn new(scale: f32, reply_to: HWND) -> Self {
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
            maker: Maker::new(reply_to),
            asked: None,
            high_surrogate: None,
            within: None,
            composing: false,
        }
    }

    // The screen around an opened image is live and no part of it, so its effects never read it.
    pub fn make_effects_from(&mut self, area: Rect) {
        self.within = Some(area);
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
        self.markup
            .editing()
            .or(self.markup.selected())
            .and_then(Swatch::of)
            .unwrap_or(self.swatch)
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
            settled: self.markup.settled(),
            previews: &self.previews,
            selected: self.markup.selected(),
            editing: self.markup.editing(),
            cursor: self.markup.cursor().unwrap_or_default(),
            composing: self.markup.composition(),
            words: &[],
            words_turn: None,
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
        if let Some(made) = self.maker.take() {
            self.accept(made);
        }
        let Some((index, asked)) = self.settle_previews() else {
            return;
        };
        let marks = self.shown_marks();
        let Some(job) = effect::jobs(&marks).nth(index) else {
            return;
        };
        let source = beneath(&job, &self.previews, scene, self.within);
        match source.map(|source| (job.mark.clone(), source)) {
            Ok((mark, source)) => {
                self.asked = Some(asked);
                self.maker.order(Order { mark, source });
            }
            Err(_) => self.refreshed = Some(self.markup.revision()),
        }
    }

    fn stale(&self) -> bool {
        !self.markup.is_busy() && self.refreshed != Some(self.markup.revision())
    }

    fn settle_previews(&mut self) -> Option<(usize, Asked)> {
        if !self.stale() {
            return None;
        }
        let marks = self.shown_marks();
        let mut shown: Vec<Preview> = Vec::new();
        let mut wanted = None;
        let mut ready = true;
        for (index, job) in effect::jobs(&marks).enumerate() {
            if !seen(&job, self.within) {
                continue;
            }
            let under: Vec<u64> = shown.iter().map(|preview| preview.stamp).collect();
            if ready && let Some(kept) = reusable(&self.previews, &job, &under) {
                shown.push(kept.clone());
                continue;
            }
            if ready && self.asked.is_none() {
                let asked = Asked {
                    width: width_key(job.mark.width),
                    beneath: job.beneath.to_vec(),
                    under,
                };
                wanted = Some((index, asked));
            }
            ready = false;
            let earlier = self.previews.iter().find(|preview| {
                preview.effect.id == job.mark.id && preview.effect.area == job.area
            });
            shown.extend(earlier.cloned());
        }
        self.previews = shown;
        if ready {
            self.refreshed = Some(self.markup.revision());
        }
        wanted
    }

    fn accept(&mut self, made: Option<Effect>) {
        let Some(asked) = self.asked.take() else {
            return;
        };
        let Some(effect) = made else {
            self.refreshed = Some(self.markup.revision());
            return;
        };
        self.stamps = self.stamps.wrapping_add(1);
        self.previews.insert(
            0,
            Preview {
                stamp: self.stamps,
                effect,
                width: asked.width,
                beneath: asked.beneath,
                under: asked.under,
            },
        );
        // The marks may have gone back while it was made, so it is checked against them again.
        self.refreshed = None;
    }

    pub fn compose(&self, image: Image, rect: Rect, scene: &Scene) -> Result<Image> {
        let shown = self.shown_marks();
        let marks = shown.as_ref();
        if marks.is_empty() {
            return Ok(image);
        }
        let mut stamps = self.stamps;
        let previews = made(marks, &self.previews, scene, self.within, &mut stamps);
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
}

fn made(
    marks: &[Mark],
    earlier: &[Preview],
    scene: &Scene,
    within: Option<Rect>,
    stamps: &mut u64,
) -> Vec<Preview> {
    let mut made: Vec<Preview> = Vec::new();
    for job in effect::jobs(marks).filter(|job| seen(job, within)) {
        let under: Vec<u64> = made.iter().map(|preview| preview.stamp).collect();
        if let Some(kept) = reusable(earlier, &job, &under) {
            made.push(kept.clone());
            continue;
        }
        let Some(effect) = beneath(&job, &made, scene, within)
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

fn beneath(job: &Job, made: &[Preview], scene: &Scene, within: Option<Rect>) -> Result<Image> {
    let area = read_area(job.area, within)
        .ok_or_else(|| Error::new(E_FAIL, "the effect lies past the image"))?;
    let mut source = scene.read(area)?;
    if !job.beneath.is_empty() {
        let gpu = scene.gpu_near(Point::new(area.left, area.top))?;
        let effects: Vec<&Effect> = made.iter().map(|preview| &preview.effect).collect();
        source = render::layered(source, area, job.beneath, &effects, gpu)?;
    }
    if area == job.area {
        return Ok(source);
    }
    effect::extended(&source, area, job.area)
        .ok_or_else(|| Error::new(E_FAIL, "the effect could not reach past the image"))
}

fn read_area(area: Rect, within: Option<Rect>) -> Option<Rect> {
    match within {
        Some(within) => area.intersection(&within),
        None => Some(area),
    }
}

// An effect wholly off an opened image is never seen, so it is neither made nor waited for.
fn seen(job: &Job, within: Option<Rect>) -> bool {
    read_area(job.area, within).is_some()
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

    fn composing(text: &str) -> Option<Composition> {
        let units: Vec<u16> = text.encode_utf16().collect();
        Composition::from_input_method(&units, &[], &[], units.len())
    }

    #[test]
    fn a_blur_over_a_text_is_made_from_the_composition_shown_in_it() {
        let mut marking = Marking::new(1.0, HWND::default());
        marking.draw(Tool::Text, Point::new(2, 2));
        marking.markup_mut().type_text("ab");
        marking.markup_mut().end_editing();
        marking.draw(Tool::Blur, Point::new(0, 0));
        marking.markup_mut().finish(Point::new(19, 19), false);
        let marks = marking.markup().marks().to_vec();
        let (Some(job), Some(text)) = (effect::jobs(&marks).next(), marks.first()) else {
            panic!("the blur lies over the text");
        };
        marking.previews = vec![made_over(&job, Vec::new())];
        assert!(marking.settle_previews().is_none());
        marking.markup_mut().edit_text(text.id);
        let asked =
            |marking: &mut Marking| marking.settle_previews().map(|(_, asked)| asked.beneath);
        let shown = |marking: &Marking| marking.shown_marks().get(..1).map(<[Mark]>::to_vec);
        for typing in ["に", "にほ"] {
            marking.markup_mut().compose(composing(typing));
            let expected = shown(&marking);
            assert_ne!(expected.as_deref(), marks.get(..1));
            assert_eq!(asked(&mut marking), expected);
        }
        marking.markup_mut().compose(None);
        assert!(marking.settle_previews().is_none());
        marking.markup_mut().compose(composing("にほ"));
        marking.markup_mut().finish_composing("日本");
        let expected = shown(&marking);
        assert_eq!(asked(&mut marking), expected);
    }

    #[test]
    fn a_blur_is_made_again_only_when_what_lies_beneath_it_changes() {
        let mut marking = Marking::new(1.0, HWND::default());
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
        let mut marking = Marking::new(1.0, HWND::default());
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
        let mut marking = Marking::new(1.0, HWND::default());
        marking.choose(Tool::Rectangle);
        marking.draw(Tool::Rectangle, Point::new(0, 0));
        marking.markup_mut().finish(Point::new(40, 30), false);
        assert!(marking.markup().selected().is_some());
        marking.choose(Tool::Arrow);
        assert!(marking.markup().selected().is_none());
    }

    #[test]
    fn choosing_the_tool_in_use_puts_it_down() {
        let mut marking = Marking::new(1.0, HWND::default());
        marking.choose(Tool::Arrow);
        assert_eq!(marking.tool(), Some(Tool::Arrow));
        marking.choose(Tool::Pen);
        assert_eq!(marking.tool(), Some(Tool::Pen));
        marking.choose(Tool::Pen);
        assert_eq!(marking.tool(), None);
    }

    #[test]
    fn the_width_follows_the_display_scale_and_stays_in_bounds() {
        let mut marking = Marking::new(2.0, HWND::default());
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
        let mut marking = Marking::new(1.0, HWND::default());
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
        let mut marking = Marking::new(1.0, HWND::default());
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
    fn a_made_blur_is_shown_until_the_one_for_its_new_width_arrives() {
        let mut marking = Marking::new(1.0, HWND::default());
        marking.draw(Tool::Blur, Point::new(0, 0));
        marking.markup_mut().finish(Point::new(9, 9), false);
        let Some(mark) = marking.markup().marks().first().cloned() else {
            panic!("the blur is drawn");
        };
        let source = Image::from_bgra(10, 10, vec![90; 400]).unwrap();
        marking.asked = Some(Asked {
            width: width_key(mark.width),
            beneath: Vec::new(),
            under: Vec::new(),
        });
        let before = marking.version();
        marking.maker.order(Order {
            mark: mark.clone(),
            source,
        });
        for _ in 0..5000 {
            if let Some(made) = marking.maker.take() {
                marking.accept(made);
            }
            if marking.asked.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(marking.previews.len(), 1);
        assert_ne!(marking.version(), before);
        let wider = Mark {
            width: mark.width + 5.0,
            ..mark.clone()
        };
        assert!(marking.decor().preview_of(&wider).is_some());
        let moved = Mark {
            shape: Shape::Blur(Point::new(5, 5), Point::new(14, 14)),
            ..mark
        };
        assert!(marking.decor().preview_of(&moved).is_none());
    }

    #[test]
    fn a_blur_made_for_a_width_undone_meanwhile_is_not_shown() {
        let mut marking = Marking::new(1.0, HWND::default());
        marking.draw(Tool::Blur, Point::new(0, 0));
        marking.markup_mut().finish(Point::new(9, 9), false);
        let Some(mark) = marking.markup().marks().first().cloned() else {
            panic!("the blur is drawn");
        };
        let made = |value: u8| Effect {
            id: mark.id,
            area: Rect::new(0, 0, 10, 10),
            image: Image::from_bgra(10, 10, vec![value; 400]).unwrap(),
        };
        marking.stamps = 1;
        marking.previews = vec![Preview {
            stamp: 1,
            effect: made(10),
            width: width_key(mark.width),
            beneath: Vec::new(),
            under: Vec::new(),
        }];
        assert!(marking.settle_previews().is_none());
        marking.widen(1);
        let Some((0, asked)) = marking.settle_previews() else {
            panic!("the blur is made again for its new width");
        };
        marking.asked = Some(asked);
        assert!(marking.markup_mut().undo());
        assert!(marking.settle_previews().is_none());
        marking.accept(Some(made(99)));
        assert!(marking.settle_previews().is_none());
        assert_eq!(marking.previews.len(), 1);
        let shown = marking
            .decor()
            .preview_of(&mark)
            .map(|preview| preview.stamp);
        assert_eq!(shown, Some(1));
    }

    #[test]
    fn a_blur_moved_off_an_opened_image_holds_up_no_other_until_it_comes_back() {
        let drawn = |within: Option<Rect>| {
            let mut marking = Marking::new(1.0, HWND::default());
            if let Some(area) = within {
                marking.make_effects_from(area);
            }
            marking.draw(Tool::Blur, Point::new(10, 10));
            marking.markup_mut().finish(Point::new(40, 40), false);
            assert!(marking.markup_mut().nudge_selected(200, 200));
            marking.draw(Tool::Pixelate, Point::new(10, 10));
            marking.markup_mut().finish(Point::new(40, 40), false);
            marking
        };
        let mut picture = drawn(Some(Rect::new(0, 0, 100, 100)));
        let Some((1, asked)) = picture.settle_previews() else {
            panic!("the pixelation on the image is made first");
        };
        let Some(pixelate) = picture.markup().marks().get(1).cloned() else {
            panic!("the pixelation is drawn");
        };
        let area = effect::area_of(&pixelate).unwrap();
        picture.asked = Some(asked);
        picture.accept(Some(Effect {
            id: pixelate.id,
            area,
            image: Image::from_bgra(
                area.width(),
                area.height(),
                vec![9; (area.width() * area.height() * 4) as usize],
            )
            .unwrap(),
        }));
        assert!(picture.settle_previews().is_none());
        assert!(!picture.stale());
        assert!(picture.decor().preview_of(&pixelate).is_some());
        assert!(picture.markup_mut().undo());
        assert!(picture.markup_mut().undo());
        assert!(matches!(picture.settle_previews(), Some((0, _))));
        let mut screenshot = drawn(None);
        assert!(matches!(screenshot.settle_previews(), Some((0, _))));
    }

    #[test]
    fn an_opened_image_reads_only_its_own_pixels_and_a_screenshot_its_whole_area() {
        let area = Rect::new(140, 70, 161, 91);
        assert_eq!(read_area(area, None), Some(area));
        assert_eq!(
            read_area(area, Some(Rect::new(50, 50, 150, 150))),
            Some(Rect::new(140, 70, 150, 91))
        );
        assert_eq!(read_area(area, Some(Rect::new(0, 0, 100, 100))), None);
    }

    #[test]
    fn a_new_colour_is_kept_for_the_next_mark() {
        let mut marking = Marking::new(1.0, HWND::default());
        marking.recolour(Swatch::Blue);
        marking.draw(Tool::Line, Point::new(0, 0));
        marking.markup_mut().finish(Point::new(10, 0), false);
        assert_eq!(
            marking.markup().marks().first().map(|mark| mark.colour),
            Some(Swatch::Blue.colour())
        );
    }
}
