//! Unit tests for the pure file-watcher decision cores. These cover the two
//! rules the watcher relies on: skip-our-own-save (now content-hash based, see
//! `is_self_write`) and debounce (`should_emit_change`).

use infinite_brainstorm_lib::{is_self_write, should_emit_change};
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

// `is_self_write` models the content-hash suppression that replaced the
// single-shot SKIP_NEXT_EMIT bool (P1.4 / F49,F93). The watcher hashes the bytes
// on disk and compares against the hash recorded by the app's last save: a match
// means our own write (skip), a mismatch means an external edit (emit).

// A tiny stand-in for the watcher's hashing step. We don't need the exact std
// hasher here — any injective-enough mapping is fine to drive the decision.
fn hash(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

#[test]
fn self_write_matching_hash_is_recognized() {
    // The app just wrote `content`; the watcher reads the same bytes back. The
    // on-disk hash matches the recorded self-write hash -> treated as our own
    // save, so the watcher will NOT emit.
    let content = r#"{"nodes":[],"edges":[]}"#;
    let last_self = Some(hash(content));
    assert!(is_self_write(hash(content), last_self));
    // And `should_emit_change` with skip=true confirms no emit.
    assert!(!should_emit_change(true, None, Instant::now(), DEBOUNCE));
}

#[test]
fn external_write_different_hash_emits() {
    // The recorded self-write hash is for the OLD content; an external editor
    // changed the file, so the on-disk hash differs -> not a self-write -> emit.
    let app_wrote = r#"{"nodes":[],"edges":[]}"#;
    let external_edit = r#"{"nodes":[{"id":"n1"}],"edges":[]}"#;
    let last_self = Some(hash(app_wrote));
    assert!(!is_self_write(hash(external_edit), last_self));
    // skip=false (not our write) with no prior emit -> emit.
    assert!(should_emit_change(false, None, Instant::now(), DEBOUNCE));
}

#[test]
fn no_prior_self_write_is_external() {
    // Before the app has ever saved, `last_self` is None: any change on disk is
    // external by definition and must reload.
    let disk = r#"{"nodes":[],"edges":[]}"#;
    assert!(!is_self_write(hash(disk), None));
}

#[test]
fn external_edit_then_resave_to_same_bytes_is_self_write() {
    // If an external edit happens to produce bytes identical to what the app
    // last wrote (same hash), it's indistinguishable from a self-write and is
    // suppressed. This is acceptable: the on-disk state already equals the app's
    // in-memory state, so reloading would be a no-op anyway.
    let content = r#"{"nodes":[],"edges":[]}"#;
    let last_self = Some(hash(content));
    assert!(is_self_write(hash(content), last_self));
}
