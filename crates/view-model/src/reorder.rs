//! Dragging a channel onto another in a server's list puts it before that
//! one, for everyone (`crates/ui/src/components/room_list.rs`).

use morpheus_core::OwnedRoomId;

/// The server's order after `from` is dropped onto `target`: its whole list
/// (`order`, then any `listed` channel it does not mention yet), with `from`
/// moved to just before `target`. `None` when nothing moves.
pub fn dropped_before(
    order: &[OwnedRoomId],
    listed: &[OwnedRoomId],
    from: &OwnedRoomId,
    target: &OwnedRoomId,
) -> Option<Vec<OwnedRoomId>> {
    if from == target {
        return None;
    }
    let mut order = order.to_vec();
    for id in listed {
        if !order.contains(id) {
            order.push(id.clone());
        }
    }
    order.retain(|id| id != from);
    let at = order.iter().position(|id| id == target).unwrap_or(order.len());
    order.insert(at, from.clone());
    Some(order)
}

/// The server's order after `from` is dropped on the `after` edge of `target`
/// (before the next channel) or its before edge: [`dropped_before`] or the
/// channel that follows `target` in the whole list.
pub fn dropped_beside(
    order: &[OwnedRoomId],
    listed: &[OwnedRoomId],
    from: &OwnedRoomId,
    target: &OwnedRoomId,
    after: bool,
) -> Option<Vec<OwnedRoomId>> {
    if from == target {
        return None;
    }
    if !after {
        return dropped_before(order, listed, from, target);
    }
    let mut all = order.to_vec();
    for id in listed {
        if !all.contains(id) {
            all.push(id.clone());
        }
    }
    all.retain(|id| id != from);
    let at = all.iter().position(|id| id == target)? + 1;
    all.insert(at, from.clone());
    (all != order).then_some(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(names: &[&str]) -> Vec<OwnedRoomId> {
        names
            .iter()
            .map(|n| OwnedRoomId::try_from(format!("!{n}:x")).expect("room id"))
            .collect()
    }

    #[test]
    fn a_channel_dropped_on_another_goes_before_it() {
        let all = ids(&["a", "b", "c"]);
        let moved = dropped_before(&all, &all, &all[2], &all[0]).expect("moves");
        assert_eq!(moved, ids(&["c", "a", "b"]));
        let moved = dropped_before(&all, &all, &all[0], &all[2]).expect("moves");
        assert_eq!(moved, ids(&["b", "a", "c"]));
    }

    #[test]
    fn dropped_on_itself_nothing_moves_and_unlisted_channels_join_the_order() {
        let all = ids(&["a", "b", "c"]);
        assert_eq!(dropped_before(&all, &all, &all[1], &all[1]), None);
        let moved = dropped_before(&all[..1], &all, &all[2], &all[1]).expect("moves");
        assert_eq!(moved, ids(&["a", "c", "b"]));
    }
}

#[cfg(test)]
mod beside_tests {
    use super::*;

    fn ids(names: &[&str]) -> Vec<OwnedRoomId> {
        names
            .iter()
            .map(|n| OwnedRoomId::try_from(format!("!{n}:x")).expect("room id"))
            .collect()
    }

    #[test]
    fn dropped_after_a_channel_goes_right_behind_it() {
        let all = ids(&["a", "b", "c"]);
        assert_eq!(dropped_beside(&all, &all, &all[0], &all[1], true), Some(ids(&["b", "a", "c"])));
        assert_eq!(dropped_beside(&all, &all, &all[0], &all[2], true), Some(ids(&["b", "c", "a"])));
        assert_eq!(dropped_beside(&all, &all, &all[2], &all[0], false), Some(ids(&["c", "a", "b"])));
        assert_eq!(dropped_beside(&all, &all, &all[0], &all[2], false), Some(ids(&["b", "a", "c"])));
    }

    #[test]
    fn dropped_where_it_already_is_moves_nothing() {
        let all = ids(&["a", "b", "c"]);
        assert_eq!(dropped_beside(&all, &all, &all[1], &all[0], true), None);
        assert_eq!(dropped_beside(&all, &all, &all[1], &all[1], true), None);
    }
}

/// A container's order after `from` is dropped onto its header: at the end
/// (`listed` channels the order does not mention yet come before it).
pub fn dropped_last(
    order: &[OwnedRoomId],
    listed: &[OwnedRoomId],
    from: &OwnedRoomId,
) -> Vec<OwnedRoomId> {
    let mut all = order.to_vec();
    for id in listed {
        if !all.contains(id) {
            all.push(id.clone());
        }
    }
    all.retain(|id| id != from);
    all.push(from.clone());
    all
}

/// The spaces a channel (in `parents`) leaves when it is dropped into `dest`:
/// every other one of the server's containers (the server and its
/// categories) it is in, so it ends up listed in one place only.
pub fn leaving(
    parents: &[OwnedRoomId],
    containers: &[OwnedRoomId],
    dest: &OwnedRoomId,
) -> Vec<OwnedRoomId> {
    parents
        .iter()
        .filter(|p| *p != dest && containers.contains(p))
        .cloned()
        .collect()
}

#[cfg(test)]
mod move_tests {
    use super::*;

    fn ids(names: &[&str]) -> Vec<OwnedRoomId> {
        names
            .iter()
            .map(|n| OwnedRoomId::try_from(format!("!{n}:x")).expect("room id"))
            .collect()
    }

    #[test]
    fn dropped_on_a_header_goes_last() {
        let all = ids(&["a", "b"]);
        assert_eq!(dropped_last(&all, &all, &ids(&["z"])[0]), ids(&["a", "b", "z"]));
        assert_eq!(dropped_last(&all, &all, &all[0]), ids(&["b", "a"]));
    }

    #[test]
    fn a_channel_dropped_into_one_container_leaves_the_others() {
        let c = ids(&["server", "cat1", "cat2"]);
        let parents = ids(&["server", "cat1", "elsewhere"]);
        assert_eq!(leaving(&parents, &c, &c[2]), ids(&["server", "cat1"]));
        assert_eq!(leaving(&parents, &c, &c[0]), ids(&["cat1"]));
        assert_eq!(leaving(&ids(&["cat1"]), &c, &c[1]), Vec::<OwnedRoomId>::new());
    }
}
