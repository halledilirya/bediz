use super::change::Rgb;

pub(super) type Channels = [f32; 3];

const SIDE: usize = 16;
const WIDTH: usize = 16;

struct Candidate {
    reach: f32,
    index: u8,
    colour: Channels,
}

pub(super) struct Nearest<'a> {
    colours: &'a [Rgb],
    cells: Vec<Option<(usize, usize)>>,
    candidates: Vec<Candidate>,
}

impl<'a> Nearest<'a> {
    pub(super) fn new(colours: &'a [Rgb]) -> Option<Self> {
        if colours.is_empty() || colours.len() > 256 {
            return None;
        }
        Some(Self {
            colours,
            cells: vec![None; SIDE * SIDE * SIDE],
            candidates: Vec::new(),
        })
    }

    pub(super) fn index(&mut self, colour: Channels) -> u8 {
        let colour = colour.map(|channel| channel.clamp(0.0, 255.0));
        let cell = cell(colour);
        let (start, end) = match self.cells.get(cell).copied().flatten() {
            Some(range) => range,
            None => {
                let range = self.collect(cell);
                if let Some(slot) = self.cells.get_mut(cell) {
                    *slot = Some(range);
                }
                range
            }
        };
        let mut best = f32::INFINITY;
        let mut chosen = 0;
        for candidate in self.candidates.get(start..end).unwrap_or_default() {
            // Reaches are whole numbers; the margin of one covers the rounding of `best`.
            if candidate.reach > best + 1.0 {
                break;
            }
            let distance = distance(colour, candidate.colour);
            if distance < best {
                best = distance;
                chosen = candidate.index;
            }
        }
        chosen
    }

    fn collect(&mut self, cell: usize) -> (usize, usize) {
        let low = corner(cell);
        let high = low.map(|channel| channel + WIDTH as i32);
        let bound = self
            .colours
            .iter()
            .map(|colour| farthest(*colour, low, high))
            .min()
            .unwrap_or(0);
        let start = self.candidates.len();
        for (index, colour) in (0..=u8::MAX).zip(self.colours) {
            let reach = closest(*colour, low, high);
            if reach <= bound {
                self.candidates.push(Candidate {
                    reach: reach as f32,
                    index,
                    colour: channels(*colour),
                });
            }
        }
        if let Some(added) = self.candidates.get_mut(start..) {
            added.sort_by(|a, b| a.reach.total_cmp(&b.reach));
        }
        (start, self.candidates.len())
    }
}

pub(super) fn channels([red, green, blue]: Rgb) -> Channels {
    [f32::from(red), f32::from(green), f32::from(blue)]
}

fn distance([red, green, blue]: Channels, [r, g, b]: Channels) -> f32 {
    let (red, green, blue) = (red - r, green - g, blue - b);
    red * red + green * green + blue * blue
}

fn cell(colour: Channels) -> usize {
    let [red, green, blue] = colour.map(|channel| (channel as usize).min(255) / WIDTH);
    (red * SIDE + green) * SIDE + blue
}

fn corner(cell: usize) -> [i32; 3] {
    [cell / (SIDE * SIDE), cell / SIDE % SIDE, cell % SIDE].map(|step| (step * WIDTH) as i32)
}

fn closest(colour: Rgb, low: [i32; 3], high: [i32; 3]) -> i32 {
    squared_gaps(colour, low, high, |value, low, high| {
        (low - value).max(value - high).max(0)
    })
}

fn farthest(colour: Rgb, low: [i32; 3], high: [i32; 3]) -> i32 {
    squared_gaps(colour, low, high, |value, low, high| {
        (value - low).max(high - value)
    })
}

fn squared_gaps(
    colour: Rgb,
    low: [i32; 3],
    high: [i32; 3],
    gap: impl Fn(i32, i32, i32) -> i32,
) -> i32 {
    colour
        .into_iter()
        .zip(low)
        .zip(high)
        .map(|((value, low), high)| gap(i32::from(value), low, high).pow(2))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Noise(u64);

    impl Noise {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, limit: u64) -> u64 {
            self.next() % limit
        }
    }

    fn clamped(colour: Channels) -> Channels {
        colour.map(|channel| channel.clamp(0.0, 255.0))
    }

    fn shortest(colours: &[Rgb], colour: Channels) -> f32 {
        colours
            .iter()
            .map(|known| distance(clamped(colour), channels(*known)))
            .fold(f32::INFINITY, f32::min)
    }

    fn palette(noise: &mut Noise, size: usize, spread: u64) -> Vec<Rgb> {
        let centre = [(); 3].map(|()| noise.below(256) as u8);
        (0..size)
            .map(|_| centre.map(|channel| channel.saturating_add(noise.below(spread) as u8)))
            .collect()
    }

    fn targets(noise: &mut Noise, colours: &[Rgb]) -> Vec<Channels> {
        (0..2_000)
            .map(|n| match n % 4 {
                0 => [(); 3].map(|()| noise.below(29_500) as f32 / 100.0 - 20.0),
                1 => [(); 3].map(|()| noise.below(256) as f32),
                2 => {
                    [(); 3].map(|()| (noise.below(17) * 16) as f32 - noise.below(2) as f32 / 1000.0)
                }
                _ => {
                    let known = colours[noise.below(colours.len() as u64) as usize];
                    channels(known).map(|channel| channel + noise.below(601) as f32 / 100.0 - 3.0)
                }
            })
            .collect()
    }

    #[test]
    fn every_colour_gets_a_palette_colour_at_the_shortest_distance() {
        for seed in 1..=60u64 {
            let mut noise = Noise(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
            let size = 1 + noise.below(255) as usize;
            let spread = [256, 64, 12][(seed % 3) as usize];
            let colours = palette(&mut noise, size, spread);
            let mut nearest = Nearest::new(&colours).unwrap();
            for target in targets(&mut noise, &colours) {
                let chosen = channels(colours[usize::from(nearest.index(target))]);
                assert_eq!(
                    distance(clamped(target), chosen),
                    shortest(&colours, target),
                    "seed {seed}, {target:?}"
                );
            }
        }
    }

    #[test]
    fn a_palette_colour_finds_itself() {
        let mut noise = Noise(7);
        let colours = palette(&mut noise, 255, 256);
        let mut nearest = Nearest::new(&colours).unwrap();
        for colour in &colours {
            assert_eq!(
                colours[usize::from(nearest.index(channels(*colour)))],
                *colour
            );
        }
    }

    #[test]
    fn colours_past_the_ends_are_taken_at_the_ends() {
        let mut noise = Noise(11);
        let colours = palette(&mut noise, 200, 256);
        let mut nearest = Nearest::new(&colours).unwrap();
        assert_eq!(
            nearest.index([-40.0, 300.0, 128.5]),
            nearest.index([0.0, 255.0, 128.5])
        );
        assert!(usize::from(nearest.index([f32::NAN, 0.0, 0.0])) < colours.len());
    }

    #[test]
    fn a_lookup_needs_one_to_two_hundred_and_fifty_six_colours() {
        assert!(Nearest::new(&[]).is_none());
        assert!(Nearest::new(&[[0, 0, 0]; 256]).is_some());
        assert!(Nearest::new(&[[0, 0, 0]; 257]).is_none());
    }
}
