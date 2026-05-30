use std::collections::VecDeque;

/// Optional tag describing the kind of edit a snapshot precedes. Successive
/// snapshots sharing the same non-`None` kind coalesce into a single undo step
/// (e.g. tapping `T` to cycle a node's type five times undoes in one stroke).
///
/// `None` (the default for one-shot actions) never coalesces, so distinct
/// operations always remain separately undoable.
pub type EditKind = Option<&'static str>;

/// History stack for undo/redo functionality.
/// Stores full state snapshots for simplicity.
///
/// Backed by [`VecDeque`] so trimming the oldest entry when `max_size` is
/// exceeded is O(1) (`pop_front`) rather than the O(n) `Vec::remove(0)`.
#[derive(Clone)]
pub struct History<T: Clone> {
    past: VecDeque<T>,
    future: VecDeque<T>,
    max_size: usize,
    /// Kind of the most recent `push` while still at the tip of the past stack.
    /// Used to coalesce successive same-kind edits. Reset to `None` whenever the
    /// timeline branches (undo/redo) so a coalesce never spans a navigation.
    last_kind: EditKind,
}

impl<T: Clone> History<T> {
    pub fn new(max_size: usize) -> Self {
        Self {
            past: VecDeque::new(),
            future: VecDeque::new(),
            max_size,
            last_kind: None,
        }
    }

    /// Record a new state without coalescing. Clears the redo stack.
    pub fn push(&mut self, state: T) {
        self.push_kind(state, None);
    }

    /// Record a new state tagged with an [`EditKind`]. Clears the redo stack.
    ///
    /// When `kind` is `Some` and equals the kind of the immediately preceding
    /// push (and we're at the tip of the timeline), the new state is *not*
    /// appended: the prior snapshot already captures the pre-edit state for the
    /// whole run, so the run collapses to one undo step. `None` never coalesces.
    pub fn push_kind(&mut self, state: T, kind: EditKind) {
        // Coalesce: a same-kind run keeps only the snapshot taken before the run
        // began. The redo stack is still cleared (a new edit invalidates redo).
        let coalesce = kind.is_some() && kind == self.last_kind && !self.past.is_empty();

        self.future.clear();
        self.last_kind = kind;

        if coalesce {
            return;
        }

        self.past.push_back(state);

        // Trim oldest entries if we exceed max size. O(1) per drop.
        while self.past.len() > self.max_size {
            self.past.pop_front();
        }
    }

    /// Undo: move current to future, return previous state.
    pub fn undo(&mut self, current: T) -> Option<T> {
        // A navigation breaks any coalescing run.
        self.last_kind = None;
        self.past.pop_back().map(|previous| {
            self.future.push_back(current);
            // Bound the redo stack the same way the undo stack is bounded, so a
            // long undo run can't grow `future` without limit.
            while self.future.len() > self.max_size {
                self.future.pop_front();
            }
            previous
        })
    }

    /// Redo: move current to past, return next state.
    pub fn redo(&mut self, current: T) -> Option<T> {
        // A navigation breaks any coalescing run.
        self.last_kind = None;
        self.future.pop_back().map(|next| {
            self.past.push_back(current);
            while self.past.len() > self.max_size {
                self.past.pop_front();
            }
            next
        })
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_history_is_empty() {
        let history: History<i32> = History::new(100);
        assert!(!history.can_undo());
        assert!(!history.can_redo());
    }

    #[test]
    fn push_enables_undo() {
        let mut history: History<i32> = History::new(100);
        history.push(1);
        assert!(history.can_undo());
        assert!(!history.can_redo());
    }

    #[test]
    fn undo_returns_previous_state() {
        let mut history: History<i32> = History::new(100);
        history.push(1);
        history.push(2);

        let result = history.undo(3);
        assert_eq!(result, Some(2));
        assert!(history.can_undo());
        assert!(history.can_redo());
    }

    #[test]
    fn redo_returns_next_state() {
        // Use distinct values to prove correctness (not coincidental)
        let mut history: History<i32> = History::new(100);
        history.push(10);
        history.push(20);

        // Current state is 30, undo to get 20
        let undone = history.undo(30);
        assert_eq!(undone, Some(20));

        // Now current is 20, redo should return 30 (what we passed to undo)
        let redone = history.redo(20);
        assert_eq!(redone, Some(30));

        // Verify: past=[10,20], future=[]
        assert!(history.can_undo());
        assert!(!history.can_redo());
    }

    #[test]
    fn push_clears_redo_stack() {
        let mut history: History<i32> = History::new(100);
        history.push(1);
        let _ = history.undo(2);
        assert!(history.can_redo());

        history.push(3);
        assert!(!history.can_redo());
    }

    #[test]
    fn respects_max_size() {
        let mut history: History<i32> = History::new(3);
        history.push(1);
        history.push(2);
        history.push(3);
        history.push(4);

        // Should only have 3 items, oldest (1) should be dropped
        assert_eq!(history.undo(5), Some(4));
        assert_eq!(history.undo(4), Some(3));
        assert_eq!(history.undo(3), Some(2));
        assert_eq!(history.undo(2), None);
    }

    #[test]
    fn undo_on_empty_returns_none() {
        let mut history: History<i32> = History::new(100);
        assert_eq!(history.undo(1), None);
    }

    #[test]
    fn redo_on_empty_returns_none() {
        let mut history: History<i32> = History::new(100);
        assert_eq!(history.redo(1), None);
    }

    #[test]
    fn chain_undo_redo() {
        let mut history: History<String> = History::new(100);
        history.push("a".to_string());
        history.push("b".to_string());
        history.push("c".to_string());

        // Current state is "d", undo to "c"
        let r1 = history.undo("d".to_string());
        assert_eq!(r1, Some("c".to_string()));

        // Undo to "b"
        let r2 = history.undo("c".to_string());
        assert_eq!(r2, Some("b".to_string()));

        // Redo back to "c"
        let r3 = history.redo("b".to_string());
        assert_eq!(r3, Some("c".to_string()));

        // Redo back to "d"
        let r4 = history.redo("c".to_string());
        assert_eq!(r4, Some("d".to_string()));

        // No more redo
        assert!(!history.can_redo());
    }

    #[test]
    fn undo_all_then_redo_all() {
        let mut history: History<i32> = History::new(100);
        history.push(1);
        history.push(2);

        // Undo all (current is 3)
        assert_eq!(history.undo(3), Some(2));
        assert_eq!(history.undo(2), Some(1));
        assert_eq!(history.undo(1), None);

        // Redo all
        assert_eq!(history.redo(1), Some(2));
        assert_eq!(history.redo(2), Some(3));
        assert_eq!(history.redo(3), None);
    }

    #[test]
    fn max_size_zero_never_stores_history() {
        let mut history: History<i32> = History::new(0);
        history.push(1);
        history.push(2);
        history.push(3);

        // Nothing should be stored
        assert!(!history.can_undo());
        assert_eq!(history.undo(4), None);
    }

    #[test]
    fn max_size_one_keeps_only_latest() {
        let mut history: History<i32> = History::new(1);
        history.push(1);
        history.push(2);
        history.push(3);

        // Only the last push should be kept
        assert_eq!(history.undo(4), Some(3));
        assert_eq!(history.undo(3), None);
    }

    #[test]
    fn future_stack_is_bounded_by_max_size() {
        // The redo stack is now bounded the same way the undo stack is, so a
        // long undo run can't grow `future` without limit.
        let mut history: History<i32> = History::new(2);
        history.push(1);
        history.push(2);
        history.push(3);
        // past is bounded to [2,3] (1 was trimmed on push of 3).

        history.undo(4); // future=[4], past=[2]
        history.undo(3); // future=[4,3], past=[]
                         // Both undos succeeded since past had 2 entries.

        // future holds 2 items (== max_size); both redos work.
        assert!(history.can_redo());
        assert_eq!(history.redo(2), Some(3));
        assert_eq!(history.redo(3), Some(4));
        assert!(!history.can_redo());
    }

    #[test]
    fn future_never_exceeds_max_size_during_full_undo_run() {
        // Drive the future stack up to exactly max_size by undoing every stored
        // entry, and confirm it stays bounded (never larger than the past was).
        let mut history: History<i32> = History::new(3);
        history.push(1);
        history.push(2);
        history.push(3); // past=[1,2,3]
        history.undo(4); // future=[4],   past=[1,2]
        history.undo(3); // future=[4,3], past=[1]
        history.undo(2); // future=[4,3,2], past=[]
                         // future holds 3 items (== max_size); all redos work, oldest preserved.
        assert!(history.can_redo());
        assert_eq!(history.redo(1), Some(2));
        assert_eq!(history.redo(2), Some(3));
        assert_eq!(history.redo(3), Some(4));
        assert!(!history.can_redo());
    }

    #[test]
    fn push_after_partial_undo_clears_only_redo() {
        let mut history: History<i32> = History::new(100);
        history.push(1);
        history.push(2);
        history.push(3);

        // Undo once (not all the way)
        history.undo(4); // past=[1,2], future=[4]

        // Push new state - should clear future but keep past
        history.push(5); // past=[1,2,5], future=[]

        assert!(!history.can_redo());

        // Should still be able to undo through original history
        assert_eq!(history.undo(6), Some(5));
        assert_eq!(history.undo(5), Some(2));
        assert_eq!(history.undo(2), Some(1));
        assert_eq!(history.undo(1), None);
    }

    #[test]
    fn multiple_undo_redo_cycles_preserve_state() {
        let mut history: History<i32> = History::new(100);
        history.push(1);
        history.push(2);

        // Cycle 1: undo then redo
        let u1 = history.undo(3);
        let r1 = history.redo(u1.unwrap());
        assert_eq!(r1, Some(3));

        // Cycle 2: same thing
        let u2 = history.undo(r1.unwrap());
        let r2 = history.redo(u2.unwrap());
        assert_eq!(r2, Some(3));

        // State should be exactly as after initial pushes
        assert!(history.can_undo());
        assert!(!history.can_redo());
    }

    // --- Coalescing (push_kind) ---

    #[test]
    fn same_kind_run_coalesces_to_one_entry() {
        let mut history: History<i32> = History::new(100);
        // Snapshot of pre-edit state before each cycle press; all same kind.
        history.push_kind(0, Some("cycle")); // captures state 0
        history.push_kind(1, Some("cycle")); // coalesced (no new entry)
        history.push_kind(2, Some("cycle")); // coalesced

        // Only one undo step back to the pre-run snapshot (0).
        assert_eq!(history.undo(3), Some(0));
        assert!(!history.can_undo());
    }

    #[test]
    fn different_kinds_do_not_coalesce() {
        let mut history: History<i32> = History::new(100);
        history.push_kind(0, Some("move"));
        history.push_kind(1, Some("cycle"));

        // Distinct kinds remain separately undoable.
        assert_eq!(history.undo(2), Some(1));
        assert_eq!(history.undo(1), Some(0));
        assert!(!history.can_undo());
    }

    #[test]
    fn none_kind_never_coalesces() {
        let mut history: History<i32> = History::new(100);
        history.push_kind(0, None);
        history.push_kind(1, None);

        assert_eq!(history.undo(2), Some(1));
        assert_eq!(history.undo(1), Some(0));
        assert!(!history.can_undo());
    }

    #[test]
    fn navigation_breaks_coalesce_run() {
        let mut history: History<i32> = History::new(100);
        history.push_kind(0, Some("cycle")); // past=[0]
        let _ = history.undo(1); // navigation resets last_kind; past=[], future=[1]
                                 // A same-kind push after navigation must NOT coalesce into the (now empty) past.
        history.push_kind(2, Some("cycle")); // past=[2], future cleared
        history.push_kind(3, Some("cycle")); // coalesced into the same run

        assert_eq!(history.undo(4), Some(2));
        assert!(!history.can_undo());
    }

    #[test]
    fn coalesce_still_clears_redo() {
        let mut history: History<i32> = History::new(100);
        history.push(1); // past=[1]
        history.undo(2); // past=[], future=[2]
                         // First push of a kind establishes the run AND clears redo.
        history.push_kind(3, Some("cycle"));
        assert!(!history.can_redo());
        // Subsequent coalesced push must also keep redo clear.
        history.push_kind(4, Some("cycle"));
        assert!(!history.can_redo());
    }
}
