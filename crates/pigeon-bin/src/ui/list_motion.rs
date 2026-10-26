//! Insert/remove motion for the message list: which rows slide in, which
//! collapse out ("ghosts" kept in place for the animation), and which row
//! must stay put on screen when the user is scrolled down.
//!
//! Only list changes within the same view animate (not the first load, a
//! mailbox switch or a search), and only small ones: a sync burst or a page
//! of older mail just appears.

use std::collections::{HashMap, HashSet};

/// More changes than this at once: no animation.
pub const MAX_ANIMATED: usize = 8;
/// A little over `--dur-collapse` (240 ms): when ghosts and entering marks go.
pub const CLEAR_AFTER_MS: u64 = 300;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Plan {
    /// New keys that slide in.
    pub entering: Vec<String>,
    /// Indexes (into `prev`) of removed rows that collapse out in place.
    pub ghosts: Vec<usize>,
    /// When scrolled down: the key whose on-screen position must not move.
    pub anchor: Option<String>,
}

impl Plan {
    pub fn animates(&self) -> bool {
        !self.entering.is_empty() || !self.ghosts.is_empty()
    }
}

/// Plan the motion from `prev` to `next` (row keys in display order).
/// `first_visible` is the index in `prev` of the first row on screen; when
/// `at_top`, everything animates and nothing needs anchoring.
pub fn plan(prev: &[String], next: &[String], same_view: bool, at_top: bool, first_visible: usize) -> Plan {
    if !same_view || prev.is_empty() || prev == next {
        return Plan::default();
    }
    let in_next: HashSet<&str> = next.iter().map(String::as_str).collect();
    let in_prev: HashSet<&str> = prev.iter().map(String::as_str).collect();
    let removed: Vec<usize> = (0..prev.len()).filter(|&i| !in_next.contains(prev[i].as_str())).collect();
    let added: Vec<usize> = (0..next.len()).filter(|&i| !in_prev.contains(next[i].as_str())).collect();

    // The row to hold still when scrolled down: the first surviving row at or
    // after the top of the viewport.
    let anchor =
        (!at_top).then(|| prev.iter().skip(first_visible).find(|k| in_next.contains(k.as_str())).cloned()).flatten();
    let quiet = Plan { anchor: anchor.clone(), ..Plan::default() };

    if removed.len() + added.len() > MAX_ANIMATED {
        return quiet;
    }
    // A page of older mail appended at the end: just appears.
    if removed.is_empty() && added.iter().all(|&i| i >= prev.len()) {
        return quiet;
    }

    let (entering, ghosts) = if at_top {
        (added.iter().map(|&i| next[i].clone()).collect(), removed)
    } else {
        // Rows above the anchor change off screen: no motion there (the
        // scroll is adjusted instead), only at or below it.
        let anchor_new = anchor.as_ref().and_then(|a| next.iter().position(|k| k == a)).unwrap_or(next.len());
        let anchor_old = anchor.as_ref().and_then(|a| prev.iter().position(|k| k == a)).unwrap_or(prev.len());
        (
            added.iter().filter(|&&i| i >= anchor_new).map(|&i| next[i].clone()).collect(),
            removed.into_iter().filter(|&i| i >= anchor_old.min(first_visible)).collect(),
        )
    };
    Plan { entering, ghosts, anchor }
}

/// Motion for the outgoing (sending/scheduled/failed) rows above the list:
/// unlike mail, the first one appearing animates too (that's the send), and
/// a send finishing (the row leaving) always collapses. A view switch just
/// shows what's there.
pub fn plan_outgoing(prev: &[String], next: &[String], same_view: bool) -> Plan {
    if !same_view || prev == next {
        return Plan::default();
    }
    let in_next: HashSet<&str> = next.iter().map(String::as_str).collect();
    let in_prev: HashSet<&str> = prev.iter().map(String::as_str).collect();
    Plan {
        entering: next.iter().filter(|k| !in_prev.contains(k.as_str())).cloned().collect(),
        ghosts: (0..prev.len()).filter(|&i| !in_next.contains(prev[i].as_str())).collect(),
        anchor: None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// Index into the new list.
    Live(usize),
    /// Index into the previous list (collapsing out).
    Ghost(usize),
}

/// Interleave ghosts with the new list: each ghost goes right after the
/// nearest earlier row that survived (or first, if none did).
pub fn merge(prev: &[String], next: &[String], ghosts: &[usize]) -> Vec<Entry> {
    let pos_next: HashMap<&str, usize> = next.iter().enumerate().map(|(i, k)| (k.as_str(), i)).collect();
    let mut after: HashMap<Option<usize>, Vec<usize>> = HashMap::new();
    for &g in ghosts {
        let anchor = (0..g).rev().find_map(|j| pos_next.get(prev[j].as_str()).copied());
        after.entry(anchor).or_default().push(g);
    }
    let mut out = Vec::with_capacity(next.len() + ghosts.len());
    out.extend(after.remove(&None).unwrap_or_default().into_iter().map(Entry::Ghost));
    for i in 0..next.len() {
        out.push(Entry::Live(i));
        if let Some(gs) = after.remove(&Some(i)) {
            out.extend(gs.into_iter().map(Entry::Ghost));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn new_mail_at_the_top_slides_in() {
        let p = plan(&k(&["a", "b", "c"]), &k(&["n", "a", "b", "c"]), true, true, 0);
        assert_eq!(p.entering, k(&["n"]));
        assert!(p.ghosts.is_empty() && p.anchor.is_none());
    }

    #[test]
    fn archived_row_collapses_in_place() {
        let prev = k(&["a", "b", "c"]);
        let next = k(&["a", "c"]);
        let p = plan(&prev, &next, true, true, 0);
        assert_eq!(p.ghosts, vec![1]);
        assert_eq!(merge(&prev, &next, &p.ghosts), vec![Entry::Live(0), Entry::Ghost(1), Entry::Live(1)]);
        // Removing the first row: the ghost leads.
        let next = k(&["b", "c"]);
        assert_eq!(merge(&prev, &next, &[0]), vec![Entry::Ghost(0), Entry::Live(0), Entry::Live(1)]);
    }

    #[test]
    fn undo_brings_the_row_back_animated() {
        let p = plan(&k(&["a", "c"]), &k(&["a", "b", "c"]), true, true, 0);
        assert_eq!(p.entering, k(&["b"]));
    }

    #[test]
    fn no_motion_for_view_changes_first_loads_bursts_or_pages() {
        let prev = k(&["a", "b"]);
        assert!(!plan(&prev, &k(&["x", "y"]), false, true, 0).animates(), "mailbox switch / search");
        assert!(!plan(&[], &prev, true, true, 0).animates(), "first load");
        let burst: Vec<String> = (0..20).map(|i| format!("n{i}")).chain(prev.clone()).collect();
        assert!(!plan(&prev, &burst, true, true, 0).animates(), "sync burst");
        assert!(!plan(&prev, &k(&["a", "b", "c", "d"]), true, true, 0).animates(), "next page appended");
        assert!(!plan(&prev, &prev, true, true, 0).animates());
    }

    #[test]
    fn outgoing_rows_slide_in_and_collapse_out() {
        let p = plan_outgoing(&[], &k(&["d1"]), true);
        assert_eq!(p.entering, k(&["d1"]), "the first send animates");
        let p = plan_outgoing(&k(&["d1", "d2"]), &k(&["d2"]), true);
        assert_eq!(p.ghosts, vec![0]);
        assert!(p.entering.is_empty());
        assert!(!plan_outgoing(&[], &k(&["d1"]), false).animates(), "view switch");
    }

    #[test]
    fn scrolled_down_anchors_and_only_animates_below() {
        // Looking at rows from "c" on: a new row arrives at the top (off
        // screen) and "e" (on screen) is archived.
        let prev = k(&["a", "b", "c", "d", "e", "f"]);
        let next = k(&["n", "a", "b", "c", "d", "f"]);
        let p = plan(&prev, &next, true, false, 2);
        assert_eq!(p.anchor.as_deref(), Some("c"));
        assert!(p.entering.is_empty(), "the new top row is off screen");
        assert_eq!(p.ghosts, vec![4]);
        // The anchor itself removed: the next surviving row holds still.
        let p = plan(&prev, &k(&["a", "b", "d", "e", "f"]), true, false, 2);
        assert_eq!(p.anchor.as_deref(), Some("d"));
        // A removal above the viewport doesn't ghost.
        let p = plan(&prev, &k(&["b", "c", "d", "e", "f"]), true, false, 2);
        assert!(p.ghosts.is_empty() && p.anchor.as_deref() == Some("c"));
    }
}
