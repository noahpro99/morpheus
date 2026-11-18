//! Applying the core's [`VectorDiff`] batches onto a plain vector: the room
//! list the desktop shell keeps and the one the phone apps (`morpheus-ffi`)
//! keep are updated the same way.

use morpheus_core::VectorDiff;

/// One change of a list from the core, applied to a plain vector. Indices
/// out of range are ignored, not panics: a stale diff must not crash the UI.
pub fn apply<T>(items: &mut Vec<T>, diff: VectorDiff<T>)
where
    T: Clone,
{
    use VectorDiff::*;
    match diff {
        Append { values } => items.extend(values),
        Clear => items.clear(),
        PushFront { value } => items.insert(0, value),
        PushBack { value } => items.push(value),
        PopFront => {
            if !items.is_empty() {
                items.remove(0);
            }
        }
        PopBack => {
            items.pop();
        }
        Insert { index, value } => items.insert(index.min(items.len()), value),
        Set { index, value } => {
            if let Some(slot) = items.get_mut(index) {
                *slot = value;
            }
        }
        Remove { index } => {
            if index < items.len() {
                items.remove(index);
            }
        }
        Truncate { length } => items.truncate(length),
        Reset { values } => *items = values.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batch_of_diffs_turns_one_list_into_the_next() {
        let mut list = vec![1, 2, 3];
        apply(&mut list, VectorDiff::PushFront { value: 0 });
        apply(&mut list, VectorDiff::Set { index: 2, value: 9 });
        apply(&mut list, VectorDiff::Remove { index: 3 });
        assert_eq!(list, [0, 1, 9]);
        apply(&mut list, VectorDiff::Remove { index: 7 });
        apply(&mut list, VectorDiff::Set { index: 7, value: 1 });
        assert_eq!(list, [0, 1, 9]);
        apply(&mut list, VectorDiff::Clear);
        assert!(list.is_empty());
    }
}
