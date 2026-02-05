# Leptos Undo/Redo with State Snapshots

## Problem

Adding undo/redo functionality to a Leptos WASM application where:
- The application has complex state (Board with nodes and edges)
- Multiple actions modify state (drag, resize, create, delete, etc.)
- State needs to persist to disk after undo/redo
- Leptos reactive views have `Send` trait requirements

## Solution

Use a **state snapshot pattern** with `Rc<RefCell<History<T>>>` for non-reactive history storage.

### Why State Snapshots Over Command Pattern

| Approach | Pros | Cons |
|----------|------|------|
| **State Snapshots** | Simple, works with all actions automatically, no command types needed | Memory usage scales with state size |
| **Command Pattern** | Memory efficient, can store diffs | Requires 13+ command types, inverse logic for each |

For small-to-medium state (~1-2KB per snapshot), snapshots win on simplicity.

### Architecture

```
┌─────────────────────────────────────────────────┐
│                   History<T>                     │
│  past: Vec<T>     ← older states                │
│  future: Vec<T>   ← states after undo           │
│                                                 │
│  push(state)  → clears future, pushes to past  │
│  undo(current) → pops past → returns previous  │
│  redo(current) → pops future → returns next    │
└─────────────────────────────────────────────────┘
```

### Implementation

#### 1. History Module (`src/history.rs`)

```rust
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

    pub fn can_undo(&self) -> bool { !self.past.is_empty() }
    pub fn can_redo(&self) -> bool { !self.future.is_empty() }
}
```

#### 2. App Integration (`src/app.rs`)

```rust
use std::cell::RefCell;
use std::rc::Rc;
use crate::history::History;

// Type alias for clarity
type BoardHistory = Rc<RefCell<History<Board>>>;

#[component]
pub fn App() -> impl IntoView {
    // Non-reactive history (doesn't need to trigger re-renders)
    let history: BoardHistory = Rc::new(RefCell::new(History::new(100)));

    // Clone for each closure that needs it
    let history_for_mouse_up = history.clone();
    let history_for_keydown = history.clone();
    // ... etc

    // Record before mutations
    let on_mouse_up = move |ev: MouseEvent| {
        if did_modify_board {
            history_for_mouse_up.borrow_mut().push(board.get_untracked());
            set_board.update(|b| { /* mutation */ });
        }
    };

    // Keyboard handlers
    let on_keydown = move |ev: KeyboardEvent| {
        match ev.key().as_str() {
            "z" if ev.meta_key() || ev.ctrl_key() => {
                ev.prevent_default();
                if ev.shift_key() {
                    // Redo: Ctrl+Shift+Z
                    if let Some(new_board) = history_for_keydown.borrow_mut()
                        .redo(board.get_untracked())
                    {
                        set_board.set(new_board.clone());
                        // Clear selections to avoid stale references
                        set_selected_nodes.set(HashSet::new());
                        set_selected_edge.set(None);
                        // Persist
                        spawn_local(async move {
                            save_board_storage(&new_board).await;
                        });
                    }
                } else {
                    // Undo: Ctrl+Z
                    if let Some(new_board) = history_for_keydown.borrow_mut()
                        .undo(board.get_untracked())
                    {
                        set_board.set(new_board.clone());
                        set_selected_nodes.set(HashSet::new());
                        set_selected_edge.set(None);
                        spawn_local(async move {
                            save_board_storage(&new_board).await;
                        });
                    }
                }
            }
            // ... other keys
        }
    };
}
```

## Critical Gotcha: Leptos Send Trait

**Problem**: Reactive views in Leptos require `Send`, but `Rc<RefCell<T>>` is not `Send`.

```rust
// THIS FAILS - reactive view requires Send
let editing_view = move || {
    let history = history.clone(); // Rc<RefCell<...>> - not Send!
    // ...
};
view! { {editing_view} }  // Error: cannot be sent between threads
```

**Solution**: Only use `Rc<RefCell<History>>` in event handler closures, NOT in reactive views.

```rust
// Event handlers are fine (not reactive)
let on_click = move |ev| {
    history.borrow_mut().push(state);  // OK
};

// Reactive views need Send - don't capture Rc<RefCell<>> here
let reactive_view = move || {
    // Don't use history here
    view! { <div>...</div> }
};
```

**Practical impact**: Text editing that uses reactive views (like inline editing with `<input>`) cannot easily record history. Workaround: accept that text edits group naturally (only saved on blur/enter anyway).

## Actions with History Recording

| Action | Where to Record | Notes |
|--------|-----------------|-------|
| Node drag | `on_mouse_up` | Only if position changed |
| Node resize | `on_mouse_up` | Only if size changed |
| Node create | `on_double_click` | Before adding node |
| Node delete | `on_keydown` | Before removing |
| Edge create | `on_mouse_up` | Before adding edge |
| Edge delete | `on_keydown` | Before removing |
| Type cycle | `on_keydown` | Before changing type |
| Image paste | `on_paste` | Before adding image node |

## Memory Considerations

- Typical Board snapshot: ~1-2KB
- Max 100 entries: ~200KB worst case
- Cleared on page reload (no persistence needed for history)
- Use `max_size` parameter to limit memory

## Testing

The history module should have comprehensive unit tests:

```rust
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
fn push_clears_redo_stack() {
    let mut history: History<i32> = History::new(100);
    history.push(1);
    let _ = history.undo(2);
    assert!(history.can_redo());

    history.push(3);
    assert!(!history.can_redo());  // Redo cleared
}
```

## Related Patterns

- **Command Pattern**: Alternative for memory-constrained scenarios
- **CRDT**: For collaborative undo (Loro, Yjs)
- **Event Sourcing**: When you need full audit trail

## Prevention Checklist

When implementing undo/redo in Leptos:

- [ ] Use `Rc<RefCell<>>` NOT signals for history (no re-render needed)
- [ ] Clone history reference for each closure that needs it
- [ ] Only use history in event handlers, not reactive views
- [ ] Record state BEFORE mutations, not after
- [ ] Clear selections after undo/redo (avoid stale node references)
- [ ] Persist to storage after undo/redo operations
- [ ] Set reasonable `max_size` limit
- [ ] Test edge cases: empty history, max size overflow, undo-then-new-action
