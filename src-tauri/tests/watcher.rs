//! Unit tests for the pure file-watcher decision core `should_emit_change`.
//! These cover the two rules the watcher relies on: skip-our-own-save (the
//! `SKIP_NEXT_EMIT` swap-consume) and debounce.

use infinite_brainstorm_lib::should_emit_change;
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(500);

#[test]
fn first_event_with_no_prior_emit_emits() {
    let now = Instant::now();
    assert!(should_emit_change(false, None, now, DEBOUNCE));
}

#[test]
fn skip_flag_suppresses_emit_even_without_prior_emit() {
    // When the app just saved (skip == true, the consumed SKIP_NEXT_EMIT), we
    // must NOT emit — otherwise the frontend reloads its own write in a loop.
    let now = Instant::now();
    assert!(!should_emit_change(true, None, now, DEBOUNCE));
}

#[test]
fn skip_flag_suppresses_emit_even_when_debounce_elapsed() {
    let base = Instant::now();
    let last = base;
    let now = base + DEBOUNCE * 2; // well past the debounce window
                                   // Debounce alone would allow emit, but the skip flag wins.
    assert!(!should_emit_change(true, Some(last), now, DEBOUNCE));
}

#[test]
fn debounce_suppresses_rapid_second_event() {
    // A second event within the debounce window after a recent emit is dropped.
    let last = Instant::now();
    let now = last + Duration::from_millis(100); // < 500ms
    assert!(!should_emit_change(false, Some(last), now, DEBOUNCE));
}

#[test]
fn emits_once_debounce_window_has_passed() {
    let last = Instant::now();
    let now = last + DEBOUNCE + Duration::from_millis(1);
    assert!(should_emit_change(false, Some(last), now, DEBOUNCE));
}

#[test]
fn debounce_boundary_is_inclusive() {
    // now == last + debounce exactly counts as elapsed (>= comparison).
    let last = Instant::now();
    let now = last + DEBOUNCE;
    assert!(should_emit_change(false, Some(last), now, DEBOUNCE));
}

#[test]
fn debounce_just_under_boundary_is_suppressed() {
    let last = Instant::now();
    let now = last + DEBOUNCE - Duration::from_millis(1);
    assert!(!should_emit_change(false, Some(last), now, DEBOUNCE));
}

#[test]
fn swap_consume_sequence_emits_then_debounces() {
    // Models the watcher loop: an external edit (skip == false, no prior emit)
    // emits; an immediate follow-up event inside the window is debounced; a
    // later event emits again.
    let t0 = Instant::now();
    assert!(
        should_emit_change(false, None, t0, DEBOUNCE),
        "external edit emits"
    );

    let last = t0;
    let t1 = t0 + Duration::from_millis(50);
    assert!(
        !should_emit_change(false, Some(last), t1, DEBOUNCE),
        "rapid follow-up is debounced"
    );

    let t2 = t0 + DEBOUNCE + Duration::from_millis(10);
    assert!(
        should_emit_change(false, Some(last), t2, DEBOUNCE),
        "event after the window emits again"
    );
}
