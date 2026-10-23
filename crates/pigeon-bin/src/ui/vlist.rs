//! Windowing math for the virtualized message list. Rows are a fixed
//! `--row-h` (76 px) and date group headers a fixed 30 px (design/screens/
//! mail-list.css), so every offset is exact without measuring the DOM.

pub const ROW_H: f64 = 76.0;
pub const GROUP_H: f64 = 30.0;
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
}

fn height(s: Slot) -> f64 {
    match s {
        Slot::Group(_) => GROUP_H,
        Slot::Item(_) => ROW_H,
    }
}

/// Lay out items given each item's group label, inserting a header whenever
/// the label changes.
pub fn layout<'a>(groups: impl IntoIterator<Item = &'static str> + 'a) -> Layout {
    let mut slots = Vec::new();
    let mut y = 0.0;
    let mut last = None;
    for (i, g) in groups.into_iter().enumerate() {
        if last != Some(g) {
            slots.push((Slot::Group(g), y));
            y += GROUP_H;
            last = Some(g);
        }
        slots.push((Slot::Item(i), y));
        y += ROW_H;
    }
    Layout { slots, total: y }
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
    let start = l.slots.partition_point(|(s, top)| top + height(*s) <= from);
    let end = l.slots.partition_point(|(_, top)| *top < to).max(start);
    let top_pad = l.slots.get(start).map(|(_, t)| *t).unwrap_or(l.total);
    let bottom_pad = match l.slots.get(end) {
        Some((_, t)) => l.total - t,
        None => 0.0,
    };
    (start, end, top_pad, bottom_pad)
}

/// Top and bottom offsets of item `i`.
pub fn item_span(l: &Layout, i: usize) -> Option<(f64, f64)> {
    l.slots.iter().find(|(s, _)| *s == Slot::Item(i)).map(|(_, t)| (*t, t + ROW_H))
}

/// Scroll position that brings `[top, bottom)` into view, or None if it is.
pub fn scroll_into_view(scroll_top: f64, viewport: f64, top: f64, bottom: f64) -> Option<f64> {
    if top < scroll_top {
        Some((top - GROUP_H).max(0.0))
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
        layout((0..n).map(|i| if i < 3 { "Today" } else { "Older" }))
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
        let rendered: f64 = l.slots[start..end].iter().map(|(s, _)| height(*s)).sum();
        assert!((top + rendered + bottom - l.total).abs() < 1e-6, "spacers + rows = total height");
        assert!(top <= 50_000.0 - OVERSCAN + ROW_H && top + rendered >= 50_800.0 + OVERSCAN - ROW_H);
        assert!(end - start < 60, "only a screenful is rendered, got {}", end - start);
        // Top of the list renders from the first slot.
        assert_eq!(window(&l, 0.0, 800.0).0, 0);
        // Past the end: nothing below.
        let (_, end, _, bottom) = window(&l, l.total, 800.0);
        assert_eq!((end, bottom), (l.slots.len(), 0.0));
        assert_eq!(window(&layout(std::iter::empty()), 0.0, 800.0), (0, 0, 0.0, 0.0));
    }

    #[test]
    fn keyboard_selection_scrolls_minimally() {
        assert_eq!(scroll_into_view(1000.0, 600.0, 1100.0, 1176.0), None);
        assert_eq!(scroll_into_view(1000.0, 600.0, 900.0, 976.0), Some(870.0));
        assert_eq!(scroll_into_view(1000.0, 600.0, 1580.0, 1656.0), Some(1064.0));
    }
}
