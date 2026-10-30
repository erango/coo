//! Windowing math for the virtualized message list. Rows and date group
//! headers have fixed heights per list style (`Metrics`; classic is the
//! 78 px edge-to-edge rows and 26 px group strips, app.css),
//! so every offset is exact without measuring the DOM.

pub const ROW_H: f64 = 78.0;
pub const GROUP_H: f64 = 26.0;

/// Slot heights of one list style (app.css sizes the rows to match).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub row_h: f64,
    pub group_h: f64,
}

pub const CLASSIC: Metrics = Metrics { row_h: ROW_H, group_h: GROUP_H };
/// Extra pixels rendered above and below the viewport.
pub const OVERSCAN: f64 = 900.0;
/// Ask for the next page when this close to the end.
pub const LOAD_AHEAD: f64 = 1800.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// A date group header ("Today", ...).
    Group(&'static str),
    /// The n-th list item.
    Item(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Each slot with its top offset, in order.
    pub slots: Vec<(Slot, f64)>,
    pub total: f64,
    pub metrics: Metrics,
}

impl Layout {
    fn height(&self, s: Slot) -> f64 {
        match s {
            Slot::Group(_) => self.metrics.group_h,
            Slot::Item(_) => self.metrics.row_h,
        }
    }

    /// Where slot `i` sits in its date group: "first", "last", "only", or
    /// None in the middle (day bands round a group's ends).
    pub fn edge(&self, i: usize) -> Option<&'static str> {
        let is_item = |j: Option<&(Slot, f64)>| matches!(j, Some((Slot::Item(_), _)));
        let first = i == 0 || !is_item(self.slots.get(i - 1));
        let last = !is_item(self.slots.get(i + 1));
        match (first, last) {
            (true, true) => Some("only"),
            (true, false) => Some("first"),
            (false, true) => Some("last"),
            (false, false) => None,
        }
    }
}

/// Lay out items given each item's group label, inserting a header whenever
/// the label changes.
pub fn layout<'a>(groups: impl IntoIterator<Item = &'static str> + 'a, metrics: Metrics) -> Layout {
    let mut slots = Vec::new();
    let mut y = 0.0;
    let mut last = None;
    for (i, g) in groups.into_iter().enumerate() {
        if last != Some(g) {
            slots.push((Slot::Group(g), y));
            y += metrics.group_h;
            last = Some(g);
        }
        slots.push((Slot::Item(i), y));
        y += metrics.row_h;
    }
    Layout { slots, total: y, metrics }
}

/// Visible slot range `[start, end)` for a scroll position, plus the spacer
/// heights above and below it.
pub fn window(l: &Layout, scroll_top: f64, viewport: f64) -> (usize, usize, f64, f64) {
    if l.slots.is_empty() {
        return (0, 0, 0.0, 0.0);
    }
    let from = (scroll_top - OVERSCAN).max(0.0);
    let to = scroll_top + viewport + OVERSCAN;
    // First slot whose bottom is below `from`.
    let start = l.slots.partition_point(|(s, top)| top + l.height(*s) <= from);
    let end = l.slots.partition_point(|(_, top)| *top < to).max(start);
    let top_pad = l.slots.get(start).map(|(_, t)| *t).unwrap_or(l.total);
    let bottom_pad = match l.slots.get(end) {
        Some((_, t)) => l.total - t,
        None => 0.0,
    };
    (start, end, top_pad, bottom_pad)
}

/// The first item at least partly visible at `scroll_top`.
pub fn first_visible_item(l: &Layout, scroll_top: f64) -> Option<usize> {
    l.slots.iter().find_map(|(s, top)| match s {
        Slot::Item(i) if top + l.metrics.row_h > scroll_top => Some(*i),
        _ => None,
    })
}

/// Top and bottom offsets of item `i`.
pub fn item_span(l: &Layout, i: usize) -> Option<(f64, f64)> {
    l.slots.iter().find(|(s, _)| *s == Slot::Item(i)).map(|(_, t)| (*t, t + l.metrics.row_h))
}

/// Scroll position that brings `[top, bottom)` into view, or None if it is.
pub fn scroll_into_view(scroll_top: f64, viewport: f64, top: f64, bottom: f64, group_h: f64) -> Option<f64> {
    if top < scroll_top {
        Some((top - group_h).max(0.0))
    } else if bottom > scroll_top + viewport {
        Some(bottom - viewport + 8.0)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(n: usize) -> Layout {
        // 3 "Today", then the rest "Older".
        layout((0..n).map(|i| if i < 3 { "Today" } else { "Older" }), CLASSIC)
    }

    #[test]
    fn offsets_include_group_headers() {
        let l = sample(5);
        assert_eq!(l.slots[0], (Slot::Group("Today"), 0.0));
        assert_eq!(l.slots[1], (Slot::Item(0), GROUP_H));
        assert_eq!(l.slots[4], (Slot::Group("Older"), GROUP_H + 3.0 * ROW_H));
        assert_eq!(l.total, 2.0 * GROUP_H + 5.0 * ROW_H);
        assert_eq!(item_span(&l, 3), Some((2.0 * GROUP_H + 3.0 * ROW_H, 2.0 * GROUP_H + 4.0 * ROW_H)));
    }

    #[test]
    fn window_covers_viewport_plus_overscan_and_spacers_add_up() {
        let l = sample(10_000);
        let (start, end, top, bottom) = window(&l, 50_000.0, 800.0);
        assert!(start > 0 && end < l.slots.len());
        let rendered: f64 = l.slots[start..end].iter().map(|(s, _)| l.height(*s)).sum();
        assert!((top + rendered + bottom - l.total).abs() < 1e-6, "spacers + rows = total height");
        assert!(top <= 50_000.0 - OVERSCAN + ROW_H && top + rendered >= 50_800.0 + OVERSCAN - ROW_H);
        assert!(end - start < 60, "only a screenful is rendered, got {}", end - start);
        // Top of the list renders from the first slot.
        assert_eq!(window(&l, 0.0, 800.0).0, 0);
        // Past the end: nothing below.
        let (_, end, _, bottom) = window(&l, l.total, 800.0);
        assert_eq!((end, bottom), (l.slots.len(), 0.0));
        assert_eq!(window(&layout(std::iter::empty(), CLASSIC), 0.0, 800.0), (0, 0, 0.0, 0.0));
    }

    #[test]
    fn first_visible_item_skips_rows_scrolled_past() {
        let l = sample(10);
        assert_eq!(first_visible_item(&l, 0.0), Some(0));
        // Past the Today header and rows 0-1 (header 30 + 2 rows), into row 2.
        assert_eq!(first_visible_item(&l, GROUP_H + 2.0 * ROW_H + 10.0), Some(2));
        assert_eq!(first_visible_item(&layout(std::iter::empty(), CLASSIC), 0.0), None);
    }

    #[test]
    fn keyboard_selection_scrolls_minimally() {
        assert_eq!(scroll_into_view(1000.0, 600.0, 1100.0, 1176.0, GROUP_H), None);
        assert_eq!(scroll_into_view(1000.0, 600.0, 900.0, 976.0, GROUP_H), Some(874.0));
        assert_eq!(scroll_into_view(1000.0, 600.0, 1580.0, 1656.0, GROUP_H), Some(1064.0));
    }

    #[test]
    fn other_styles_and_group_edges() {
        let m = Metrics { row_h: 60.0, group_h: 34.0 };
        let l = layout(["Today", "Today", "Older"], m);
        assert_eq!(l.total, 2.0 * 34.0 + 3.0 * 60.0);
        assert_eq!(item_span(&l, 2), Some((2.0 * 34.0 + 2.0 * 60.0, 2.0 * 34.0 + 3.0 * 60.0)));
        // Slots: Group, Item0, Item1, Group, Item2.
        assert_eq!((l.edge(1), l.edge(2), l.edge(4)), (Some("first"), Some("last"), Some("only")));
        let l = layout(["Today"; 3], CLASSIC);
        assert_eq!(l.edge(2), None);
    }
}
