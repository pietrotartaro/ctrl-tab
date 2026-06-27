//! Switcher state + the pure, unit-tested logic.
//!
//! Pure (TDD): `wrapping_advance`, `filter_eligible`, `promote_mru`,
//! `order_by_mru`. The `Switcher` struct is side-effecting glue (logging),
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

/// A raw running application as seen during enumeration (before filtering).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawApp {
    pub pid: i32,
    pub name: String,
    /// True iff activationPolicy == .regular.
    pub regular: bool,
}

/// An eligible app shown in the switcher. Icons are kept in a separate pid-keyed
/// cache, not here, so the pure ordering logic stays trivial to test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppItem {
    pub pid: i32,
    pub name: String,
}

/// Keep only `.regular` apps and drop our own process.
pub fn filter_eligible(apps: Vec<RawApp>, own_pid: i32) -> Vec<RawApp> {
    apps.into_iter()
        .filter(|a| a.regular && a.pid != own_pid)
        .collect()
}

/// Move `pid` to the front of the MRU order (most-recent-first), preserving the
/// relative order of everything else. Inserts it if not already present.
pub fn promote_mru(order: &mut Vec<i32>, pid: i32) {
    order.retain(|&p| p != pid);
    order.insert(0, pid);
}

/// Order `apps` so the pids listed in `mru` come first (in `mru` order), followed
/// by any apps not in `mru` in their original enumeration order.
pub fn order_by_mru(apps: Vec<AppItem>, mru: &[i32]) -> Vec<AppItem> {
    use std::collections::HashMap;

    let original_order: Vec<i32> = apps.iter().map(|a| a.pid).collect();
    let mut by_pid: HashMap<i32, AppItem> = apps.into_iter().map(|a| (a.pid, a)).collect();

    let mut result = Vec::with_capacity(original_order.len());
    for &pid in mru {
        if let Some(a) = by_pid.remove(&pid) {
            result.push(a);
        }
    }
    for pid in original_order {
        if let Some(a) = by_pid.remove(&pid) {
            result.push(a);
        }
    }
    result
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

/// In-memory switcher state driven by the event tap.
pub struct Switcher {
    pub active: bool,
    pub mode: Mode,
    pub selected: usize,
    pub items: Vec<AppItem>,
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

    /// Begin a gesture with a prebuilt, already-ordered item list. `selected` is
    /// clamped to the list bounds.
    pub fn start(&mut self, mode: Mode, items: Vec<AppItem>, selected: usize) {
        self.active = true;
        self.mode = mode;
        self.items = items;
        self.selected = if self.items.is_empty() {
            0
        } else {
            selected.min(self.items.len() - 1)
        };
        let name = self
            .items
            .get(self.selected)
            .map(|a| a.name.as_str())
            .unwrap_or("<empty>");
        crate::dlog!(
            "[ctl-tab] gesture_start  mode={:<7} items={} selected={} -> {}",
            mode.label(),
            self.items.len(),
            self.selected,
            name
        );
        for (i, a) in self.items.iter().enumerate() {
            crate::dlog!(
                "[ctl-tab]   [{}] pid={:<6} {}{}",
                i,
                a.pid,
                a.name,
                if i == self.selected { "  <" } else { "" }
            );
        }
    }

    /// Move the selection by `delta` (+1 right / -1 left), wrapping. No-op if idle.
    pub fn advance(&mut self, delta: isize) {
        if !self.active {
            return;
        }
        self.selected = wrapping_advance(self.selected, delta, self.items.len());
        let name = self
            .items
            .get(self.selected)
            .map(|a| a.name.as_str())
            .unwrap_or("<empty>");
        crate::dlog!(
            "[ctl-tab] advance        mode={:<7} dir={:+} selected={}/{} -> {}",
            self.mode.label(),
            delta.signum(),
            self.selected,
            self.items.len(),
            name
        );
    }

    /// Confirm the current selection and reset. Returns the selected item (for the
    /// caller to activate). No-op / None if idle or empty.
    pub fn commit(&mut self) -> Option<AppItem> {
        if !self.active {
            return None;
        }
        let item = self.items.get(self.selected).cloned();
        match &item {
            Some(a) => crate::dlog!(
                "[ctl-tab] commit         mode={:<7} selected={} pid={} {}",
                self.mode.label(),
                self.selected,
                a.pid,
                a.name
            ),
            None => crate::dlog!(
                "[ctl-tab] commit         mode={:<7} selected={} <empty>",
                self.mode.label(),
                self.selected
            ),
        }
        self.reset();
        item
    }

    /// Abort without selecting and reset. No-op if idle.
    pub fn cancel(&mut self) {
        if !self.active {
            return;
        }
        crate::dlog!(
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
    use super::*;

    // ---- wrapping_advance ----

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
        assert_eq!(wrapping_advance(0, 7, 5), 2);
        assert_eq!(wrapping_advance(4, -3, 5), 1);
        assert_eq!(wrapping_advance(0, -7, 5), 3);
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

    // ---- filter_eligible ----

    fn raw(pid: i32, name: &str, regular: bool) -> RawApp {
        RawApp {
            pid,
            name: name.into(),
            regular,
        }
    }

    #[test]
    fn filter_keeps_regular_apps() {
        let apps = vec![raw(1, "Safari", true), raw(2, "Notes", true)];
        let out = filter_eligible(apps, 999);
        assert_eq!(out, vec![raw(1, "Safari", true), raw(2, "Notes", true)]);
    }

    #[test]
    fn filter_drops_non_regular_apps() {
        let apps = vec![
            raw(1, "Safari", true),
            raw(2, "menubar-agent", false),
            raw(3, "Notes", true),
        ];
        let out = filter_eligible(apps, 999);
        assert_eq!(out, vec![raw(1, "Safari", true), raw(3, "Notes", true)]);
    }

    #[test]
    fn filter_drops_own_process() {
        let apps = vec![raw(1, "Safari", true), raw(42, "ctl-tab", true)];
        let out = filter_eligible(apps, 42);
        assert_eq!(out, vec![raw(1, "Safari", true)]);
    }

    // ---- promote_mru ----

    #[test]
    fn promote_new_pid_goes_to_front() {
        let mut order = vec![2, 3];
        promote_mru(&mut order, 1);
        assert_eq!(order, vec![1, 2, 3]);
    }

    #[test]
    fn promote_existing_pid_moves_to_front_preserving_rest() {
        let mut order = vec![1, 2, 3, 4];
        promote_mru(&mut order, 3);
        assert_eq!(order, vec![3, 1, 2, 4]);
    }

    #[test]
    fn promote_front_pid_stays_front_without_dupes() {
        let mut order = vec![1, 2, 3];
        promote_mru(&mut order, 1);
        assert_eq!(order, vec![1, 2, 3]);
    }

    // ---- order_by_mru ----

    fn item(pid: i32, name: &str) -> AppItem {
        AppItem {
            pid,
            name: name.into(),
        }
    }

    #[test]
    fn order_puts_mru_pids_first_in_mru_order() {
        let apps = vec![item(1, "A"), item(2, "B"), item(3, "C")];
        let out = order_by_mru(apps, &[3, 1]);
        assert_eq!(out, vec![item(3, "C"), item(1, "A"), item(2, "B")]);
    }

    #[test]
    fn order_with_empty_mru_keeps_original_order() {
        let apps = vec![item(1, "A"), item(2, "B"), item(3, "C")];
        let out = order_by_mru(apps, &[]);
        assert_eq!(out, vec![item(1, "A"), item(2, "B"), item(3, "C")]);
    }

    #[test]
    fn order_ignores_mru_pids_not_present() {
        let apps = vec![item(1, "A"), item(2, "B")];
        let out = order_by_mru(apps, &[99, 2]);
        assert_eq!(out, vec![item(2, "B"), item(1, "A")]);
    }

    // ---- smoke test: the pure pipeline the native app-list build uses ----

    #[test]
    fn eligible_then_mru_ordered_pipeline() {
        let own_pid = 42;
        let raw_apps = vec![
            raw(1, "Safari", true),
            raw(2, "menubar-agent", false), // dropped (not regular)
            raw(3, "Notes", true),
            raw(42, "ctl-tab", true), // dropped (own pid)
            raw(4, "Mail", true),
        ];
        let mru = vec![4, 1]; // Mail most recent, then Safari

        let items: Vec<AppItem> = filter_eligible(raw_apps, own_pid)
            .into_iter()
            .map(|r| AppItem {
                pid: r.pid,
                name: r.name,
            })
            .collect();
        let ordered = order_by_mru(items, &mru);

        // MRU first (Mail, Safari), then the rest in enumeration order (Notes).
        assert_eq!(
            ordered,
            vec![item(4, "Mail"), item(1, "Safari"), item(3, "Notes")]
        );
    }
}
