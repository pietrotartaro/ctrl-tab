//! Switcher state.
//!
//! `wrapping_advance` is the only pure, unit-tested piece (per the TDD policy).
//! The `Switcher` struct is the side-effecting glue that drives logging; it is
//! exercised by the manual acceptance criteria, not unit tests.

/// Advance `index` by `delta` over a list of length `len`, wrapping around in
/// both directions. `delta` is signed: +1 moves right (forward), -1 left (back).
/// Returns 0 for an empty list.
pub fn wrapping_advance(index: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let n = len as isize;
    ((index as isize + delta).rem_euclid(n)) as usize
}

/// Which list the switcher is cycling through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Apps,
    Windows,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Apps => "apps",
            Mode::Windows => "windows",
        }
    }
}

/// In-memory switcher state driven by the event tap. Phase 1 only logs and works
/// off a fake list of 5 items — no real app/window enumeration yet.
pub struct Switcher {
    pub active: bool,
    pub mode: Mode,
    pub selected: usize,
    pub items: Vec<String>,
}

impl Switcher {
    pub fn new() -> Self {
        Self {
            active: false,
            mode: Mode::Apps,
            selected: 0,
            items: Vec::new(),
        }
    }

    /// Begin a gesture: populate the (fake) list and select the current item (0).
    pub fn start(&mut self, mode: Mode) {
        self.active = true;
        self.mode = mode;
        self.selected = 0;
        self.items = (0..5).map(|i| format!("item-{i}")).collect();
        eprintln!(
            "[ctl-tab] gesture_start  mode={:<7} selected={} items={}",
            mode.label(),
            self.selected,
            self.items.len()
        );
    }

    /// Move the selection by `delta` (+1 right / -1 left), wrapping. No-op if idle.
    pub fn advance(&mut self, delta: isize) {
        if !self.active {
            return;
        }
        self.selected = wrapping_advance(self.selected, delta, self.items.len());
        eprintln!(
            "[ctl-tab] advance        mode={:<7} dir={:+} selected={}/{}",
            self.mode.label(),
            delta.signum(),
            self.selected,
            self.items.len()
        );
    }

    /// Confirm the current selection and reset. No-op if idle.
    pub fn commit(&mut self) {
        if !self.active {
            return;
        }
        let item = self.items.get(self.selected).cloned().unwrap_or_default();
        eprintln!(
            "[ctl-tab] commit         mode={:<7} selected={} item=\"{}\"",
            self.mode.label(),
            self.selected,
            item
        );
        self.reset();
    }

    /// Abort without selecting and reset. No-op if idle.
    pub fn cancel(&mut self) {
        if !self.active {
            return;
        }
        eprintln!(
            "[ctl-tab] cancel         mode={:<7} selected={}",
            self.mode.label(),
            self.selected
        );
        self.reset();
    }

    fn reset(&mut self) {
        self.active = false;
        self.selected = 0;
        self.items.clear();
    }
}

impl Default for Switcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::wrapping_advance;

    #[test]
    fn forward_within_bounds() {
        assert_eq!(wrapping_advance(0, 1, 5), 1);
        assert_eq!(wrapping_advance(2, 1, 5), 3);
    }

    #[test]
    fn forward_wraps_past_end() {
        assert_eq!(wrapping_advance(4, 1, 5), 0);
    }

    #[test]
    fn backward_wraps_past_start() {
        assert_eq!(wrapping_advance(0, -1, 5), 4);
        assert_eq!(wrapping_advance(2, -1, 5), 1);
    }

    #[test]
    fn large_deltas_wrap_modularly() {
        assert_eq!(wrapping_advance(0, 7, 5), 2); // 7 mod 5
        assert_eq!(wrapping_advance(4, -3, 5), 1); // (4-3) mod 5
        assert_eq!(wrapping_advance(0, -7, 5), 3); // (-7) mod 5 == 3
    }

    #[test]
    fn single_element_list_stays_put() {
        assert_eq!(wrapping_advance(0, 1, 1), 0);
        assert_eq!(wrapping_advance(0, -1, 1), 0);
    }

    #[test]
    fn empty_list_returns_zero() {
        assert_eq!(wrapping_advance(0, 1, 0), 0);
        assert_eq!(wrapping_advance(0, -1, 0), 0);
    }
}
