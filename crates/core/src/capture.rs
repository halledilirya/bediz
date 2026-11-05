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
}
