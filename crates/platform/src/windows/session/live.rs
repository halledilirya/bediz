//! The live screen's new frames on each monitor: shown at once, or left to the redraw that input is
//! about to make, so a monitor being drawn on is presented once a frame, not twice.

#[derive(Debug, Default)]
pub struct LiveFrames {
    unshown: Vec<bool>,
    redrawn: Vec<bool>,
}

impl LiveFrames {
    pub fn arrived(&mut self, monitors: &[usize], input_waiting: bool) -> Vec<usize> {
        for &index in monitors {
            set(&mut self.unshown, index, true);
        }
        let shown: Vec<usize> = self
            .unshown
            .iter()
            .enumerate()
            .filter(|&(index, unshown)| {
                *unshown && !(input_waiting && self.redrawn.get(index) == Some(&true))
            })
            .map(|(index, _)| index)
            .collect();
        for &index in &shown {
            set(&mut self.unshown, index, false);
        }
        self.redrawn.fill(false);
        shown
    }

    pub fn owed(&self, monitor: usize) -> bool {
        self.unshown.get(monitor) == Some(&true)
    }

    pub fn drawn(&mut self, monitor: usize) {
        set(&mut self.unshown, monitor, false);
        set(&mut self.redrawn, monitor, true);
    }
}

fn set(flags: &mut Vec<bool>, index: usize, value: bool) {
    if flags.len() <= index {
        flags.resize(index + 1, false);
    }
    if let Some(flag) = flags.get_mut(index) {
        *flag = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_frames_are_shown_at_once_while_no_input_waits() {
        let mut frames = LiveFrames::default();
        assert_eq!(frames.arrived(&[0, 1], false), vec![0, 1]);
        assert!(!frames.owed(0) && !frames.owed(1));
        frames.drawn(0);
        assert_eq!(frames.arrived(&[0], false), vec![0]);
        assert_eq!(frames.arrived(&[], false), Vec::<usize>::new());
    }

    #[test]
    fn a_monitor_input_keeps_redrawing_takes_its_frame_with_the_next_redraw() {
        let mut frames = LiveFrames::default();
        frames.drawn(0);
        assert_eq!(frames.arrived(&[0, 1], true), vec![1]);
        assert!(frames.owed(0));
        frames.drawn(0);
        assert!(!frames.owed(0));
        assert_eq!(frames.arrived(&[], true), Vec::<usize>::new());
    }

    #[test]
    fn a_frame_left_to_a_redraw_that_never_came_is_shown_with_the_next() {
        let mut frames = LiveFrames::default();
        frames.drawn(2);
        assert_eq!(frames.arrived(&[2], true), Vec::<usize>::new());
        assert_eq!(frames.arrived(&[], true), vec![2]);
        assert!(!frames.owed(2));
    }
}
