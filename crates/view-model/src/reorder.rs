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
