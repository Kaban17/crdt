//! Tests of typed validation of protocol invariants (`ValidateMerge`).
//!
//! Two groups:
//!
//! * property tests "valid histories never produce an error": a
//!   history is valid when each actor lives on exactly one replica
//!   (and, for LWW, the value of a write is a deterministic function
//!   of its stamp);
//! * hand-written scenarios that break each invariant: the error must
//!   be detected and carry a precise witness of the violation.

use crdt_lab::{lww, mvreg, orset, JoinSemilattice, LwwRegister, MvRegister, OrSet, ValidateMerge};
use proptest::collection::vec;
use proptest::prelude::*;

// --- Valid histories: no errors ---

proptest! {
    /// LWW: actors are split across replicas, the value is a function
    /// of the stamp.
    #[test]
    fn lww_valid_histories_never_error(
        ts_a in vec(0u64..40, 0..16),
        ts_b in vec(0u64..40, 0..16),
    ) {
        let mut a = LwwRegister::new("a", 0u64);
        let mut b = LwwRegister::new("b", 0u64);
        for ts in ts_a {
            a.write(ts, "a", ts);
        }
        for ts in ts_b {
            b.write(ts, "b", ts);
        }
        prop_assert!(a.validate_merge(&b).is_ok());
        let merged = a.clone().joined(&b);
        prop_assert!(merged.validate_merge(&a).is_ok());
        prop_assert!(merged.validate_merge(&b).is_ok());
    }

    /// OR-Set: each replica issues tags only from its own actor;
    /// midway through the history there is an exchange of states.
    #[test]
    fn orset_valid_histories_never_error(
        ops1_a in set_ops(), ops1_b in set_ops(),
        ops2_a in set_ops(), ops2_b in set_ops(),
    ) {
        let mut a: OrSet<&str, u8> = OrSet::new();
        let mut b: OrSet<&str, u8> = OrSet::new();
        run_set(&mut a, "a", &ops1_a);
        run_set(&mut b, "b", &ops1_b);
        prop_assert!(a.validate_merge(&b).is_ok());

        let merged = a.clone().joined(&b);
        a = merged.clone();
        b = merged;
        run_set(&mut a, "a", &ops2_a);
        run_set(&mut b, "b", &ops2_b);
        prop_assert!(a.validate_merge(&b).is_ok());
        prop_assert!(b.validate_merge(&a).is_ok());
    }

    /// MV-register: each replica writes from its own actor;
    /// midway through the history there is an exchange of states.
    #[test]
    fn mvreg_valid_histories_never_error(
        vals1_a in vec(any::<u8>(), 0..8), vals1_b in vec(any::<u8>(), 0..8),
        vals2_a in vec(any::<u8>(), 0..8), vals2_b in vec(any::<u8>(), 0..8),
    ) {
        let mut a: MvRegister<&str, u8> = MvRegister::new();
        let mut b: MvRegister<&str, u8> = MvRegister::new();
        for v in vals1_a {
            a.write("a", v);
        }
        for v in vals1_b {
            b.write("b", v);
        }
        prop_assert!(a.validate_merge(&b).is_ok());

        let merged = a.clone().joined(&b);
        a = merged.clone();
        b = merged;
        for v in vals2_a {
            a.write("a", v);
        }
        for v in vals2_b {
            b.write("b", v);
        }
        prop_assert!(a.validate_merge(&b).is_ok());
        prop_assert!(b.validate_merge(&a).is_ok());
    }
}

#[derive(Debug, Clone)]
enum SetOp {
    Add(u8),
    Remove(u8),
}

fn set_ops() -> impl Strategy<Value = Vec<SetOp>> {
    vec(
        prop_oneof![
            (0u8..6).prop_map(SetOp::Add),
            (0u8..6).prop_map(SetOp::Remove),
        ],
        0..12,
    )
}

fn run_set(set: &mut OrSet<&'static str, u8>, actor: &'static str, ops: &[SetOp]) {
    for op in ops {
        match op {
            SetOp::Add(x) => set.add(actor, *x),
            SetOp::Remove(x) => set.remove(x),
        }
    }
}

// --- Broken invariants: an error with a precise witness ---

/// LWW: one actor issued one timestamp to two different values.
#[test]
fn lww_detects_stamp_reuse() {
    let mut a = LwwRegister::new("x", "");
    let mut b = a.clone();
    a.write(1, "x", "v1");
    b.write(1, "x", "v2");
    let err = a.validate_merge(&b).unwrap_err();
    assert_eq!(
        err,
        lww::MergeError {
            timestamp: 1,
            actor: "x",
            values: ("v1", "v2"),
        }
    );
}

/// LWW: matching full state (including the value) is not a violation.
#[test]
fn lww_identical_stamps_same_value_are_valid() {
    let mut a = LwwRegister::new("x", "");
    a.write(1, "x", "v");
    let b = a.clone();
    assert!(a.validate_merge(&b).is_ok());
}

/// OR-Set: one actor generated tags on two replicas: a Dot collision.
#[test]
fn orset_detects_dot_collision() {
    let mut a: OrSet<&str, &str> = OrSet::new();
    let mut b: OrSet<&str, &str> = OrSet::new();
    a.add("x", "apple");
    b.add("x", "banana"); // the same actor from zero → Dot("x", 1) again
    let err = a.validate_merge(&b).unwrap_err();
    assert_eq!(*err.dot.actor(), "x");
    assert_eq!(err.dot.counter(), 1);
    assert_eq!(err.items, ("apple", "banana"));
}

/// OR-Set: a shared history (one tag on one element) is not a violation.
#[test]
fn orset_shared_history_is_valid() {
    let mut a: OrSet<&str, &str> = OrSet::new();
    a.add("x", "apple");
    let mut b = a.clone();
    b.remove(&"apple");
    assert!(a.validate_merge(&b).is_ok());
    assert!(b.validate_merge(&a).is_ok());
}

/// MV-register: one actor issued one event number to two values —
/// equal clocks with different values.
#[test]
fn mvreg_detects_clock_collision() {
    let mut a: MvRegister<&str, i32> = MvRegister::new();
    let mut b: MvRegister<&str, i32> = MvRegister::new();
    a.write("x", 1);
    b.write("x", 2); // both states carry the clock {x: 1}
    let err = a.validate_merge(&b).unwrap_err();
    assert_eq!(err.clock.get(&"x"), 1);
    assert_eq!(err.values, (1, 2));
}

/// Errors are proper `std::error::Error` values with a meaningful message.
#[test]
fn merge_errors_implement_error() {
    fn check(err: &(impl std::error::Error + ?Sized)) {
        assert!(!err.to_string().is_empty());
    }

    check(&lww::MergeError {
        timestamp: 1,
        actor: "x",
        values: ("v1", "v2"),
    });
    let mut a: MvRegister<&str, i32> = MvRegister::new();
    let mut b: MvRegister<&str, i32> = MvRegister::new();
    a.write("x", 1);
    b.write("x", 2);
    check(&a.validate_merge(&b).unwrap_err() as &mvreg::MergeError<_, _>);

    let mut sa: OrSet<&str, &str> = OrSet::new();
    let mut sb: OrSet<&str, &str> = OrSet::new();
    sa.add("x", "apple");
    sb.add("x", "banana");
    check(&sa.validate_merge(&sb).unwrap_err() as &orset::MergeError<_, _>);
}
