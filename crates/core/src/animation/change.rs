pub(super) type Rgb = [u8; 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Area {
    pub(super) left: u16,
    pub(super) top: u16,
    pub(super) width: u16,
    pub(super) height: u16,
}

impl Area {
    pub(super) fn whole(width: u16, height: u16) -> Self {
        Self {
            left: 0,
            top: 0,
            width,
            height,
        }
    }
}

pub(super) struct Cut {
    pub(super) pixels: Vec<Rgb>,
    pub(super) changed: Vec<bool>,
}

pub(super) fn changed_area(previous: &[Rgb], current: &[Rgb], width: u16) -> Option<Area> {
    let row = usize::from(width);
    if row == 0 {
        return None;
    }
    let mut bounds: Option<(usize, usize, usize, usize)> = None;
    for (y, (before, after)) in previous
        .chunks_exact(row)
        .zip(current.chunks_exact(row))
        .enumerate()
    {
        if before == after {
            continue;
        }
        let pairs = || before.iter().zip(after);
        let (Some(first), Some(last)) = (
            pairs().position(|(a, b)| a != b),
            pairs().rposition(|(a, b)| a != b),
        ) else {
            continue;
        };
        bounds = Some(match bounds {
            None => (first, y, last, y),
            Some((left, top, right, _)) => (left.min(first), top, right.max(last), y),
        });
    }
    let (left, top, right, bottom) = bounds?;
    Some(Area {
        left: u16::try_from(left).ok()?,
        top: u16::try_from(top).ok()?,
        width: u16::try_from(right - left + 1).ok()?,
        height: u16::try_from(bottom - top + 1).ok()?,
    })
}

pub(super) fn cut(previous: Option<&[Rgb]>, current: &[Rgb], width: u16, area: Area) -> Cut {
    let pixel_count = usize::from(area.width) * usize::from(area.height);
    let mut cut = Cut {
        pixels: Vec::with_capacity(pixel_count),
        changed: Vec::with_capacity(pixel_count),
    };
    for y in usize::from(area.top)..usize::from(area.top) + usize::from(area.height) {
        let start = y * usize::from(width) + usize::from(area.left);
        let range = start..start + usize::from(area.width);
        let Some(row) = current.get(range.clone()) else {
            continue;
        };
        cut.pixels.extend_from_slice(row);
        match previous.and_then(|previous| previous.get(range)) {
            Some(before) => cut
                .changed
                .extend(row.iter().zip(before).map(|(now, then)| now != then)),
            None => cut.changed.extend(row.iter().map(|_| true)),
        }
    }
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Rgb = [0, 0, 0];
    const RED: Rgb = [255, 0, 0];

    fn with(changes: &[(usize, usize)]) -> Vec<Rgb> {
        let mut pixels = vec![BLACK; 5 * 4];
        for (x, y) in changes {
            pixels[y * 5 + x] = RED;
        }
        pixels
    }

    #[test]
    fn identical_frames_have_no_changed_area() {
        assert_eq!(changed_area(&with(&[]), &with(&[]), 5), None);
    }

    #[test]
    fn one_changed_pixel_is_its_own_area() {
        assert_eq!(
            changed_area(&with(&[]), &with(&[(2, 1)]), 5),
            Some(Area {
                left: 2,
                top: 1,
                width: 1,
                height: 1
            })
        );
    }

    #[test]
    fn the_area_bounds_every_change() {
        assert_eq!(
            changed_area(&with(&[]), &with(&[(3, 0), (1, 2), (4, 3)]), 5),
            Some(Area {
                left: 1,
                top: 0,
                width: 4,
                height: 4
            })
        );
    }

    #[test]
    fn a_zero_width_has_no_area() {
        assert_eq!(changed_area(&with(&[]), &with(&[(0, 0)]), 0), None);
    }

    #[test]
    fn a_cut_marks_only_the_changed_pixels() {
        let current = with(&[(1, 1), (3, 2)]);
        let area = changed_area(&with(&[]), &current, 5).unwrap();
        let cut = cut(Some(&with(&[])), &current, 5, area);
        assert_eq!(cut.pixels, [RED, BLACK, BLACK, BLACK, BLACK, RED]);
        assert_eq!(cut.changed, [true, false, false, false, false, true]);
    }

    #[test]
    fn without_a_previous_frame_every_pixel_has_changed() {
        let current = with(&[(0, 0)]);
        let cut = cut(None, &current, 5, Area::whole(5, 4));
        assert_eq!(cut.pixels, current);
        assert!(cut.changed.iter().all(|changed| *changed));
    }
}
