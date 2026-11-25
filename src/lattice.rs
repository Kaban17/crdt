//! Core of the library: the join-semilattice contract and derived notions.
//!
//! The Rust type system carries the algebraic specification: the
//! [`JoinSemilattice`] trait fixes the signature of `⊔`, and its laws
//! (associativity, commutativity, idempotence) are checked by generic
//! property-based tests written once for any `T: JoinSemilattice`.

/// Replica (actor) identifier.
///
/// A trait alias that collects the requirements on the identifier type.
/// `Ord` is needed for deterministic collections (`BTreeMap`/`BTreeSet`)
/// and conflict resolution; `Clone` is needed to store the id in several
/// components of the state.
pub trait Actor: Ord + Clone {}

impl<A: Ord + Clone> Actor for A {}

/// Join-semilattice: a type with a join operation `⊔`.
///
/// # Contract (checked by tests, not by the compiler)
///
/// For any `a`, `b`, `c`:
///
/// * `(a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)` — associativity;
/// * `a ⊔ b = b ⊔ a` — commutativity;
/// * `a ⊔ a = a` — idempotence.
///
/// The laws are not expressible in Rust's type system (no dependent
/// types). Responsibility is split accordingly: *signatures* and
/// *derived definitions* live in the trait; *laws* live in generic
/// property-based tests (`tests/laws.rs`).
///
/// # Derived order
///
/// The partial order is not implemented by hand: [`leq`](Self::leq) is
/// defined from `join` by the classical identity `a ≤ b ⟺ a ⊔ b = b`.
/// The order and the merge therefore agree by construction.
pub trait JoinSemilattice: Clone + Eq {
    /// The `⊔` operation: replaces `self` with `self ⊔ other`.
    ///
    /// Takes `other` by shared reference: merge must not mutate the
    /// other replica, and the signature already enforces that.
    fn join(&mut self, other: &Self);

    /// Owning form of `⊔`: convenient in folds and tests.
    #[must_use]
    fn joined(mut self, other: &Self) -> Self {
        self.join(other);
        self
    }

    /// Partial order induced by `join`: `a ≤ b ⟺ a ⊔ b = b`.
    fn leq(&self, other: &Self) -> bool {
        self.clone().joined(other) == *other
    }
}

/// Local updates of a CvRDT.
///
/// Shapiro et al. require operations to be **inflationary**:
/// `a ≤ apply(a, op)` for any operation. That is the second hypothesis
/// of the convergence theorem, after the semilattice laws. Putting
/// operations in the associated type [`Op`](Self::Op) makes the
/// equation generic: like the laws of `⊔`, it is written once for any
/// `T: Update` and instantiated by a macro (`tests/laws.rs`,
/// `inflation_law!`).
///
/// A side effect is that generating arbitrary *reachable* states in
/// tests becomes universal: a state is a fold of a list of operations
/// (`tests/common/mod.rs`, function `build`).
pub trait Update: JoinSemilattice {
    /// Type of a local operation.
    type Op;

    /// Apply a local operation to the state.
    fn apply(&mut self, op: Self::Op);
}

/// Validation of protocol invariants before merge.
///
/// Rust's type system statically guarantees part of the CvRDT
/// specification (signatures, no mutation of the other replica), but
/// *protocol* invariants — properties of well-formed histories, not of
/// individual states — are out of reach: uniqueness of LWW timestamps,
/// uniqueness of [`Dot`] tags in an OR-Set, injectivity of
/// "clock → value" for the MV-register. Breaking them does not break
/// the semilattice laws (those hold for *any* states), but it makes the
/// converged value meaningless.
///
/// This trait lifts those invariants into the API as **typed errors**:
/// before `join`, a pair of states is checked for contradiction, and a
/// violation is returned as a value of the associated type
/// [`Error`](Self::Error) rather than discovered silently after merge.
/// The limit of the approach is visible in which types implement it:
/// for `GCounter`/`GSet` a contradiction between two states is
/// unobservable in principle (any pair of states is a legal pair of
/// replicas), so the trait is not implemented for them.
///
/// [`Dot`]: crate::orset::Dot
pub trait ValidateMerge: JoinSemilattice {
    /// Witness of an invariant violation: what exactly contradicts.
    type Error;

    /// Check a pair of states against protocol invariants.
    ///
    /// `Ok(())` does not mean "the states are well formed", only that
    /// this pair contains no *observable* contradiction. The relation
    /// is symmetric up to the orientation of the witness.
    fn validate_merge(&self, other: &Self) -> Result<(), Self::Error>;
}

/// State-based CRDT (CvRDT).
///
/// Merging replica states is exactly the semilattice `⊔`, so the trait
/// is implemented *automatically* (blanket impl) for any semilattice:
/// in this model "being a CvRDT" and "being a semilattice with
/// inflationary updates" are the same thing. Inflation of updates is
/// the responsibility of each concrete type and of separate tests.
pub trait CvRdt: JoinSemilattice {
    /// Merge another replica's state into this one.
    fn merge(&mut self, other: &Self) {
        self.join(other);
    }
}

impl<T: JoinSemilattice> CvRdt for T {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal semilattice for the derived-order test: max on u64.
    #[derive(Clone, PartialEq, Eq, Debug)]
    struct MaxU64(u64);

    impl JoinSemilattice for MaxU64 {
        fn join(&mut self, other: &Self) {
            self.0 = self.0.max(other.0);
        }
    }

    #[test]
    fn le_derived_from_join() {
        assert!(MaxU64(1).leq(&MaxU64(2)));
        assert!(!MaxU64(2).leq(&MaxU64(1)));
        assert!(MaxU64(2).leq(&MaxU64(2)));
    }

    #[test]
    fn merge_is_join() {
        let mut a = MaxU64(1);
        a.merge(&MaxU64(5));
        assert_eq!(a, MaxU64(5));
    }
}
