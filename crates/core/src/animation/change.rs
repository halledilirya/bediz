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

pub(super) fn differs(now: Rgb, then: Rgb, threshold: u8) -> bool {
    now.iter()
        .zip(then)
        .any(|(now, then)| now.abs_diff(then) > threshold)
}

pub(super) fn changed_area(
    previous: &[Rgb],
    current: &[Rgb],
    width: u16,
    threshold: u8,
) -> Option<Area> {
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
            pairs().position(|(a, b)| differs(*b, *a, threshold)),
            pairs().rposition(|(a, b)| differs(*b, *a, threshold)),
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

pub(super) fn cut(
    previous: Option<&[Rgb]>,
    current: &[Rgb],
    width: u16,
    area: Area,
    threshold: u8,
) -> Cut {
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
            Some(before) => cut.changed.extend(
                row.iter()
                    .zip(before)
                    .map(|(now, then)| differs(*now, *then, threshold)),
            ),
            None => cut.changed.extend(row.iter().map(|_| true)),
        }
    }
    cut
}

pub(super) fn remember(
    reference: &mut [Rgb],
    current: &[Rgb],
    width: u16,
    area: Area,
    changed: &[bool],
) {
    let span = usize::from(area.width);
    if span == 0 {
        return;
    }
    for (y, flags) in (usize::from(area.top)..).zip(changed.chunks_exact(span)) {
        let start = y * usize::from(width) + usize::from(area.left);
        let (Some(kept), Some(now)) = (
            reference.get_mut(start..start + span),
            current.get(start..start + span),
        ) else {
            continue;
        };
        for ((kept, now), changed) in kept.iter_mut().zip(now).zip(flags) {
            if *changed {
                *kept = *now;
            }
        }
    }
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
        assert_eq!(changed_area(&with(&[]), &with(&[]), 5, 0), None);
    }

    #[test]
    fn one_changed_pixel_is_its_own_area() {
        assert_eq!(
            changed_area(&with(&[]), &with(&[(2, 1)]), 5, 0),
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
            changed_area(&with(&[]), &with(&[(3, 0), (1, 2), (4, 3)]), 5, 0),
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
        assert_eq!(changed_area(&with(&[]), &with(&[(0, 0)]), 0, 0), None);
    }

    #[test]
    fn a_cut_marks_only_the_changed_pixels() {
        let current = with(&[(1, 1), (3, 2)]);
        let area = changed_area(&with(&[]), &current, 5, 0).unwrap();
        let cut = cut(Some(&with(&[])), &current, 5, area, 0);
        assert_eq!(cut.pixels, [RED, BLACK, BLACK, BLACK, BLACK, RED]);
        assert_eq!(cut.changed, [true, false, false, false, false, true]);
    }

    #[test]
    fn without_a_previous_frame_every_pixel_has_changed() {
        let current = with(&[(0, 0)]);
        let cut = cut(None, &current, 5, Area::whole(5, 4), 0);
        assert_eq!(cut.pixels, current);
        assert!(cut.changed.iter().all(|changed| *changed));
    }

    #[test]
    fn a_change_counts_only_past_the_threshold_in_some_channel() {
        assert!(!differs([10, 20, 30], [12, 18, 30], 2));
        assert!(differs([10, 20, 30], [10, 20, 33], 2));
        assert!(differs([10, 20, 30], [11, 20, 30], 0));
        assert!(!differs([10, 20, 30], [10, 20, 30], 0));
    }

    #[test]
    fn the_area_and_the_cut_leave_out_changes_within_the_threshold() {
        let mut current = with(&[(3, 2)]);
        current[5 + 1] = [2, 2, 2];
        let area = changed_area(&with(&[]), &current, 5, 2).unwrap();
        assert_eq!(
            area,
            Area {
                left: 3,
                top: 2,
                width: 1,
                height: 1
            }
        );
        let wide = Area {
            left: 1,
            top: 1,
            width: 3,
            height: 2,
        };
        let cut = cut(Some(&with(&[])), &current, 5, wide, 2);
        assert_eq!(cut.changed, [false, false, false, false, false, true]);
    }

    #[test]
    fn only_changed_pixels_are_remembered() {
        let mut reference = with(&[]);
        let mut current = with(&[(1, 1), (2, 1)]);
        current[5 + 3] = [1, 1, 1];
        let area = Area {
            left: 1,
            top: 1,
            width: 3,
            height: 1,
        };
        remember(&mut reference, &current, 5, area, &[true, false, false]);
        assert_eq!(reference, with(&[(1, 1)]));
    }
}
