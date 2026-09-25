//! Undo/redo over element changes: napkin's own design, not a port of Excalidraw's
//! id-keyed, per-property `StoreDelta`/`HistoryDelta`. Each recorded step is a snapshot diff
//! by position rather than id, storing each changed position's element before and after
//! verbatim (including `version`/`versionNonce`/`updated`) so undo and redo restore it
//! exactly rather than recomputing it from a property delta; an insertion or removal at the
//! end of the element list still replays correctly in both directions, since `None` on either
//! side of a [`Change`] marks a position that did not exist in that state.
//!
//! Only elements are recorded; `appState` (the view) is not history's concern here, since
//! napkin keeps scroll and zoom in the app layer rather than restoring them on undo.

use std::collections::VecDeque;

use crate::element::Element;
use crate::file::SceneFile;
use crate::selection::Selection;

/// Spec §5.7.
pub const HISTORY_LIMIT: usize = 200;

/// One position's element before and after the recorded change; `None` on either side means
/// the position did not exist in that state (an insertion or removal at the end).
#[derive(Clone, Debug, PartialEq)]
struct Change {
    position: usize,
    before: Option<Element>,
    after: Option<Element>,
}

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    changes: Vec<Change>,
    selection_before: Selection,
    selection_after: Selection,
}

#[derive(Debug, Default)]
pub struct History {
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
}

/// The positions where `before` and `after` disagree, ascending. A length difference between
/// the two only ever shows up as trailing positions present on one side and absent on the
/// other, since every position under both lengths is compared directly.
fn diff(before: &SceneFile, after: &SceneFile) -> Vec<Change> {
    let len = before.elements.len().max(after.elements.len());
    (0..len)
        .filter_map(|position| {
            let before_element = before.elements.get(position).cloned();
            let after_element = after.elements.get(position).cloned();
            (before_element != after_element).then_some(Change {
                position,
                before: before_element,
                after: after_element,
            })
        })
        .collect()
}

/// Replays `changes` against `file`, taking each change's `side` (`before` for undo, `after`
/// for redo). A change whose side holds an element either overwrites that position, when it is
/// still in range, or is pushed onto the end, when it is not (restoring an element that had
/// been removed); a change whose side is empty truncates the vector down to that position
/// (dropping an element that had been inserted). Changes are applied in ascending position
/// order, so a run of trailing pushes or truncations lands at the right final length.
fn apply(file: &mut SceneFile, changes: &[Change], side: impl Fn(&Change) -> &Option<Element>) {
    for change in changes {
        match side(change) {
            Some(element) => {
                if change.position < file.elements.len() {
                    file.elements[change.position] = element.clone();
                } else {
                    file.elements.push(element.clone());
                }
            }
            None => file.elements.truncate(change.position),
        }
    }
}

impl History {
    /// Records the element changes from `before` to `after`, matched by position; a length
    /// difference is an insertion or removal at the end. Returns `false` (recording nothing,
    /// keeping redo) when no element changed. Otherwise clears redo and drops the oldest entry
    /// beyond `HISTORY_LIMIT`.
    pub fn record(
        &mut self,
        before: &SceneFile,
        after: &SceneFile,
        selection_before: &Selection,
        selection_after: &Selection,
    ) -> bool {
        let changes = diff(before, after);
        if changes.is_empty() {
            return false;
        }
        self.redo.clear();
        self.undo.push_back(Entry {
            changes,
            selection_before: selection_before.clone(),
            selection_after: selection_after.clone(),
        });
        while self.undo.len() > HISTORY_LIMIT {
            self.undo.pop_front();
        }
        true
    }

    /// Restores the elements of the newest entry and returns its `selection_before`.
    pub fn undo(&mut self, file: &mut SceneFile) -> Option<Selection> {
        let entry = self.undo.pop_back()?;
        apply(file, &entry.changes, |change| &change.before);
        let selection = entry.selection_before.clone();
        self.redo.push(entry);
        Some(selection)
    }

    /// Reapplies the newest undone entry and returns its `selection_after`.
    pub fn redo(&mut self, file: &mut SceneFile) -> Option<Selection> {
        let entry = self.redo.pop()?;
        apply(file, &entry.changes, |change| &change.after);
        let selection = entry.selection_after.clone();
        self.undo.push_back(entry);
        Some(selection)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample;

    fn at(x: f64) -> SceneFile {
        sample::file(vec![sample::generic(
            "rectangle",
            "r",
            [x, 0.0, 10.0, 10.0],
        )])
    }

    #[test]
    fn undo_and_redo_swap_states_and_selection() {
        let (before, after) = (at(0.0), at(10.0));
        let mut history = History::default();
        assert!(history.record(
            &before,
            &after,
            &Selection::new(),
            &Selection::from_ids(["r"])
        ));
        let mut file = after.clone();
        assert_eq!(history.undo(&mut file), Some(Selection::new()));
        assert_eq!(file, before);
        assert!(!history.can_undo() && history.can_redo());
        assert_eq!(history.redo(&mut file), Some(Selection::from_ids(["r"])));
        assert_eq!(file, after);
        assert_eq!(history.redo(&mut file), None);
    }

    #[test]
    fn appended_elements_disappear_on_undo() {
        let before = at(0.0);
        let mut after = before.clone();
        after.elements.push(scene_element("n"));
        let mut history = History::default();
        history.record(
            &before,
            &after,
            &Selection::new(),
            &Selection::from_ids(["n"]),
        );
        let mut file = after.clone();
        history.undo(&mut file);
        assert_eq!(file, before);
        history.redo(&mut file);
        assert_eq!(file, after);
    }

    fn scene_element(id: &str) -> crate::Element {
        crate::Element::from_value(sample::generic("ellipse", id, [0.0, 0.0, 5.0, 5.0]))
    }

    #[test]
    fn no_change_records_nothing_and_new_records_clear_redo() {
        let mut history = History::default();
        assert!(!history.record(
            &at(0.0),
            &at(0.0),
            &Selection::new(),
            &Selection::from_ids(["r"])
        ));
        assert!(!history.can_undo());
        history.record(&at(0.0), &at(1.0), &Selection::new(), &Selection::new());
        let mut file = at(1.0);
        history.undo(&mut file);
        assert!(history.can_redo());
        history.record(&at(0.0), &at(2.0), &Selection::new(), &Selection::new());
        assert!(!history.can_redo());
    }

    #[test]
    fn keeps_the_latest_two_hundred_steps() {
        let mut history = History::default();
        for i in 0..(HISTORY_LIMIT + 5) {
            history.record(
                &at(i as f64),
                &at(i as f64 + 1.0),
                &Selection::new(),
                &Selection::new(),
            );
        }
        let mut file = at((HISTORY_LIMIT + 5) as f64);
        let mut undone = 0;
        while history.undo(&mut file).is_some() {
            undone += 1;
        }
        assert_eq!(undone, HISTORY_LIMIT);
        assert_eq!(file, at(5.0));
    }
}
