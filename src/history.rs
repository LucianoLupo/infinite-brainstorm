/// History stack for undo/redo functionality.
/// Stores full state snapshots for simplicity.
#[derive(Clone)]
pub struct History<T: Clone> {
    past: Vec<T>,
    future: Vec<T>,
    max_size: usize,
}

impl<T: Clone> History<T> {
    pub fn new(max_size: usize) -> Self {
        Self {
            past: Vec::new(),
            future: Vec::new(),
            max_size,
        }
    }

    /// Record a new state. Clears the redo stack.
    pub fn push(&mut self, state: T) {
        self.future.clear();
        self.past.push(state);

        // Trim oldest entries if we exceed max size
        while self.past.len() > self.max_size {
            self.past.remove(0);
        }
    }

    /// Undo: move current to future, return previous state.
    pub fn undo(&mut self, current: T) -> Option<T> {
        self.past.pop().map(|previous| {
            self.future.push(current);
            previous
        })
    }

    /// Redo: move current to past, return next state.
    pub fn redo(&mut self, current: T) -> Option<T> {
        self.future.pop().map(|next| {
            self.past.push(current);
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
    fn future_stack_grows_on_repeated_undo() {
        // Documents behavior: future stack is not trimmed by max_size
        let mut history: History<i32> = History::new(3);
        history.push(1);
        history.push(2);
        history.push(3);

        // Undo all - future stack will have 3 items
        history.undo(4);
        history.undo(3);
        history.undo(2);

        // All 3 redos should work
        assert!(history.can_redo());
        assert_eq!(history.redo(1), Some(2));
        assert_eq!(history.redo(2), Some(3));
        assert_eq!(history.redo(3), Some(4));
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
}
