use std::fmt;
use std::num::NonZeroU32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureMethod {
    Duplication,
    Gdi,
}

impl CaptureMethod {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Duplication => "duplication",
            Self::Gdi => "gdi",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pixels {
    #[default]
    Bgra8,
    Linear,
    Hdr {
        sdr_white_level: NonZeroU32,
    },
}

impl Pixels {
    // scRGB's 1.0 is 80 nits, and Windows gives the SDR white level in thousandths of it.
    pub fn scale(self) -> f32 {
        match self {
            Self::Hdr { sdr_white_level } => 1000.0 / sdr_white_level.get() as f32,
            _ => 1.0,
        }
    }
}

impl fmt::Display for Pixels {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bgra8 => formatter.write_str("bgra8"),
            Self::Linear => formatter.write_str("linear"),
            Self::Hdr { sdr_white_level } => {
                write!(
                    formatter,
                    "hdr-{}nits",
                    u64::from(sdr_white_level.get()) * 80 / 1000
                )
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Allocation {
    #[default]
    Ahead,
    OnHotkey,
}

impl Allocation {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ahead => "ahead",
            Self::OnHotkey => "on-hotkey",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique() {
        let methods = [CaptureMethod::Duplication, CaptureMethod::Gdi].map(CaptureMethod::name);
        assert_eq!(methods, ["duplication", "gdi"]);
        let allocations = [Allocation::Ahead, Allocation::OnHotkey].map(Allocation::name);
        assert_eq!(allocations, ["ahead", "on-hotkey"]);
    }

    #[test]
    fn hdr_pixels_are_scaled_so_sdr_white_becomes_one() {
        let hdr = |level| Pixels::Hdr {
            sdr_white_level: NonZeroU32::new(level).unwrap(),
        };
        assert_eq!(hdr(1000).scale(), 1.0);
        assert_eq!(hdr(2500).scale(), 0.4);
        assert_eq!(hdr(4000).scale(), 0.25);
        assert_eq!(Pixels::Linear.scale(), 1.0);
        assert_eq!(Pixels::Bgra8.scale(), 1.0);
    }

    #[test]
    fn pixels_name_the_sdr_white_in_nits() {
        assert_eq!(Pixels::Bgra8.to_string(), "bgra8");
        assert_eq!(Pixels::Linear.to_string(), "linear");
        assert_eq!(
            Pixels::Hdr {
                sdr_white_level: NonZeroU32::new(3000).unwrap()
            }
            .to_string(),
            "hdr-240nits"
        );
    }
}
