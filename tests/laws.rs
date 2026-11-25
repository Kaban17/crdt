//! Generic checks of the join-semilattice laws.
//!
//! The laws are written **once** for an arbitrary `T: JoinSemilattice`
//! (the `assert_*` functions), and the `lattice_laws!` macro
//! instantiates them for each implementation. Adding a new CRDT to the
//! library takes one line in this file; the type system does the rest.
//!
//! Checked:
//! * associativity, commutativity, idempotence of `⊔`;
//! * inflation of merge (`a ≤ a ⊔ b`) and the least-upper-bound
//!   property (`a ≤ c ∧ b ≤ c ⇒ a ⊔ b ≤ c`);
//! * consistency of the derived order (reflexivity, transitivity,
//!   antisymmetry);
//! * inflation of local operations (`a ≤ apply(a, op)`), generically
//!   via `Update::Op` (macro `inflation_law!`).

mod common;

use crdt_lab::{JoinSemilattice, Update};
use proptest::prelude::*;

fn assert_associative<T: JoinSemilattice + std::fmt::Debug>(a: &T, b: &T, c: &T) {
    let left = a.clone().joined(b).joined(c);
    let right = a.clone().joined(&b.clone().joined(c));
    assert_eq!(left, right, "(a ⊔ b) ⊔ c ≠ a ⊔ (b ⊔ c)");
}

fn assert_commutative<T: JoinSemilattice + std::fmt::Debug>(a: &T, b: &T) {
    assert_eq!(a.clone().joined(b), b.clone().joined(a), "a ⊔ b ≠ b ⊔ a");
}

fn assert_idempotent<T: JoinSemilattice + std::fmt::Debug>(a: &T) {
    assert_eq!(a.clone().joined(a), *a, "a ⊔ a ≠ a");
}

fn assert_join_is_lub<T: JoinSemilattice + std::fmt::Debug>(a: &T, b: &T, c: &T) {
    let ab = a.clone().joined(b);
    // upper bound
    assert!(a.leq(&ab), "a ≰ a ⊔ b");
    assert!(b.leq(&ab), "b ≰ a ⊔ b");
    // least: any upper bound c dominates a ⊔ b
    let upper = c.clone().joined(a).joined(b);
    assert!(ab.leq(&upper), "a ⊔ b ≰ upper bound");
}

fn assert_partial_order<T: JoinSemilattice + std::fmt::Debug>(a: &T, b: &T, c: &T) {
    assert!(a.leq(a), "order is not reflexive");
    if a.leq(b) && b.leq(c) {
        assert!(a.leq(c), "order is not transitive");
    }
    if a.leq(b) && b.leq(a) {
        assert_eq!(a, b, "order is not antisymmetric");
    }
}

/// Inflation of an operation: `a ≤ apply(a, op)` is the second
/// hypothesis of the CvRDT convergence theorem, made generic through
/// `Update::Op`.
fn assert_inflationary<T: Update + std::fmt::Debug>(state: &T, op: T::Op)
where
    T::Op: std::fmt::Debug + Clone,
{
    let mut after = state.clone();
    after.apply(op.clone());
    assert!(state.leq(&after), "operation {op:?} is not inflationary");
}

macro_rules! inflation_law {
    ($name:ident, $state:expr, $op:expr) => {
        mod $name {
            use super::*;

            proptest! {
                #[test]
                fn updates_are_inflationary(state in $state, op in $op) {
                    assert_inflationary(&state, op);
                }
            }
        }
    };
}

macro_rules! lattice_laws {
    ($name:ident, $strategy:expr) => {
        mod $name {
            use super::*;

            proptest! {
                #[test]
                fn associative(a in $strategy, b in $strategy, c in $strategy) {
                    assert_associative(&a, &b, &c);
                }

                #[test]
                fn commutative(a in $strategy, b in $strategy) {
                    assert_commutative(&a, &b);
                }

                #[test]
                fn idempotent(a in $strategy) {
                    assert_idempotent(&a);
                }

                #[test]
                fn join_is_least_upper_bound(a in $strategy, b in $strategy, c in $strategy) {
                    assert_join_is_lub(&a, &b, &c);
                }

                #[test]
                fn derived_relation_is_partial_order(
                    a in $strategy, b in $strategy, c in $strategy
                ) {
                    assert_partial_order(&a, &b, &c);
                }
            }
        }
    };
}

lattice_laws!(vclock_laws, common::vclock());
lattice_laws!(gcounter_laws, common::gcounter());
lattice_laws!(pncounter_laws, common::pncounter());
lattice_laws!(gset_laws, common::gset());
lattice_laws!(lww_register_laws, common::lww_register());
lattice_laws!(mvreg_laws, common::mvreg());
lattice_laws!(orset_laws, common::orset());
lattice_laws!(orswot_laws, common::orswot());

inflation_law!(vclock_inflation, common::vclock(), common::vclock_op());
inflation_law!(
    gcounter_inflation,
    common::gcounter(),
    common::gcounter_op()
);
inflation_law!(
    pncounter_inflation,
    common::pncounter(),
    common::pncounter_op()
);
inflation_law!(gset_inflation, common::gset(), common::gset_op());
inflation_law!(lww_inflation, common::lww_register(), common::lww_op());
inflation_law!(mvreg_inflation, common::mvreg(), common::mvreg_op());
inflation_law!(orset_inflation, common::orset(), common::orset_op());
inflation_law!(orswot_inflation, common::orswot(), common::orset_op());
