//! Dragging a server badge on the rail (`RAIL_DRAG_JS` in
//! `crates/ui/src/components/rail.rs`): where a drop at a given height goes.

/// What a drop over one rail entry does, by where in the entry it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    /// Just above it: before the entry.
    Before,
    /// Its middle: onto it (a folder of the two, or into the folder).
    Onto,
    /// Just below it: before the next entry.
    After,
}

/// Where the pointer at `y` is over an entry spanning `top..bottom`: the top
/// quarter and anything above is before it, the bottom quarter and anything
/// below after it, the middle onto it.
pub fn zone(y: f32, top: f32, bottom: f32) -> Zone {
    let quarter = (bottom - top) * 0.25;
    if y < top + quarter {
        Zone::Before
    } else if y <= bottom - quarter {
        Zone::Onto
    } else {
        Zone::After
    }
}

/// Where a drop goes in the rail's entries: the index it lands before, and
/// whether it lands onto that entry instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drop {
    pub index: usize,
    pub onto: bool,
}

/// The drop over entry `index` with the pointer in `zone`. A badge held over
/// its own entry's middle drops nowhere (`own`).
pub fn target(index: usize, zone: Zone, own: bool) -> Option<Drop> {
    match zone {
        Zone::Before => Some(Drop { index, onto: false }),
        Zone::After => Some(Drop {
            index: index + 1,
            onto: false,
        }),
        Zone::Onto if own => None,
        Zone::Onto => Some(Drop { index, onto: true }),
    }
}

/// For a list of rows that cannot be dropped onto (channels): which edge of the
/// row at `top..bottom` the pointer at `y` is nearer, with the same hysteresis
/// on both lists: a pointer within the middle fifth of the row keeps the edge
/// it had (`previous`), so the line does not flicker as it crosses.
pub fn edge(y: f32, top: f32, bottom: f32, previous: Option<Zone>) -> Zone {
    let (mid, band) = ((top + bottom) / 2.0, (bottom - top) * 0.1);
    match previous {
        Some(zone @ (Zone::Before | Zone::After)) if (y - mid).abs() <= band => zone,
        _ if y < mid => Zone::Before,
        _ => Zone::After,
    }
}

/// How far from a list's top or bottom edge (px) the pointer starts scrolling
/// it, and the scroll in px per frame at the very edge.
pub const SCROLL_EDGE: f32 = 36.0;
pub const SCROLL_SPEED: f32 = 14.0;

/// The scroll step for a pointer at `y` over a list spanning `top..bottom`:
/// negative up, positive down, 0 away from the edges, faster the nearer.
pub fn auto_scroll(y: f32, top: f32, bottom: f32) -> f32 {
    if y < top + SCROLL_EDGE {
        -SCROLL_SPEED * ((top + SCROLL_EDGE - y) / SCROLL_EDGE).min(1.0)
    } else if y > bottom - SCROLL_EDGE {
        SCROLL_SPEED * ((y - (bottom - SCROLL_EDGE)) / SCROLL_EDGE).min(1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_edges_are_gaps_and_the_middle_is_onto() {
        assert_eq!(zone(100.0, 100.0, 148.0), Zone::Before);
        assert_eq!(zone(111.0, 100.0, 148.0), Zone::Before);
        assert_eq!(zone(124.0, 100.0, 148.0), Zone::Onto);
        assert_eq!(zone(136.0, 100.0, 148.0), Zone::Onto);
        assert_eq!(zone(137.0, 100.0, 148.0), Zone::After);
        assert_eq!(zone(90.0, 100.0, 148.0), Zone::Before, "above it: before");
        assert_eq!(zone(160.0, 100.0, 148.0), Zone::After, "below it: after");
    }

    #[test]
    fn a_drop_after_an_entry_is_before_the_next() {
        assert_eq!(target(2, Zone::Before, false), Some(Drop { index: 2, onto: false }));
        assert_eq!(target(2, Zone::After, false), Some(Drop { index: 3, onto: false }));
        assert_eq!(target(2, Zone::Onto, false), Some(Drop { index: 2, onto: true }));
    }

    #[test]
    fn a_badge_held_over_its_own_middle_goes_nowhere() {
        assert_eq!(target(2, Zone::Onto, true), None);
        assert!(target(2, Zone::Before, true).is_some());
    }
}

#[cfg(test)]
mod edge_tests {
    use super::*;

    #[test]
    fn the_nearer_edge_wins_and_the_middle_keeps_the_last() {
        assert_eq!(edge(102.0, 100.0, 140.0, None), Zone::Before);
        assert_eq!(edge(138.0, 100.0, 140.0, None), Zone::After);
        assert_eq!(edge(121.0, 100.0, 140.0, Some(Zone::Before)), Zone::Before);
        assert_eq!(edge(119.0, 100.0, 140.0, Some(Zone::After)), Zone::After);
        assert_eq!(edge(125.0, 100.0, 140.0, Some(Zone::Before)), Zone::After, "past the band");
    }

    #[test]
    fn the_list_scrolls_near_its_edges_only() {
        assert_eq!(auto_scroll(300.0, 0.0, 600.0), 0.0);
        assert!(auto_scroll(10.0, 0.0, 600.0) < 0.0);
        assert!(auto_scroll(590.0, 0.0, 600.0) > 0.0);
        assert!(auto_scroll(0.0, 0.0, 600.0) < auto_scroll(30.0, 0.0, 600.0));
    }
}
