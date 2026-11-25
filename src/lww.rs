//! LWW-Register: last-writer-wins register.

use std::fmt;

use crate::lattice::{Actor, JoinSemilattice, Update, ValidateMerge};

/// Register that resolves conflicts by *last-writer-wins*.
///
/// State is a triple `(timestamp, actor, value)`; `join` picks the
/// lexicographically larger triple. The lexicographic order on a
/// product of **linear** orders is linear, and `max` in a linear order
/// is the join of a semilattice, so the laws hold automatically.
///
/// Conflict resolution is deterministic at the type level:
///
/// * on equal timestamps the larger actor identifier wins
///   (`A: Ord` is required by the type system: a register with
///   incomparable actors simply does not compile);
/// * a fallback third component `T: Ord` makes `join` total even when
///   the protocol invariant "the pair `(timestamp, actor)` is unique"
///   is broken. The semilattice laws hold for *any* states, which
///   simplifies and strengthens the property-based tests.
///
/// The semantic cost of LWW is lost updates: of concurrent writes,
/// only one survives. That is a property of the type, not a defect of
/// the implementation.
///
/// # Example
///
/// ```
/// use crdt_lab::{LwwRegister, CvRdt};
///
/// let mut a = LwwRegister::new("a", "draft");
/// let mut b = a.clone();
/// a.write(1, "a", "version A");
/// b.write(2, "b", "version B");
///
/// a.merge(&b);
/// b.merge(&a);
/// assert_eq!(a, b);
/// assert_eq!(*a.value(), "version B"); // larger timestamp won
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LwwRegister<A: Actor, T: Ord + Clone> {
    timestamp: u64,
    actor: A,
    value: T,
}

impl<A: Actor, T: Ord + Clone> LwwRegister<A, T> {
    /// Register with an initial value and timestamp 0.
    pub fn new(actor: A, value: T) -> Self {
        Self {
            timestamp: 0,
            actor,
            value,
        }
    }

    /// Current value.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Timestamp of the current value.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Actor that set the current value.
    pub fn actor(&self) -> &A {
        &self.actor
    }

    /// Write a value with a timestamp.
    ///
    /// Implemented as `join` with a singleton state, so inflation holds
    /// by construction: a stale write (with a smaller triple) simply
    /// does not change the register.
    pub fn write(&mut self, timestamp: u64, actor: A, value: T) {
        self.join(&Self {
            timestamp,
            actor,
            value,
        });
    }

    /// Comparison key of the state.
    fn key(&self) -> (u64, &A, &T) {
        (self.timestamp, &self.actor, &self.value)
    }
}

/// Initial state from default values (timestamp 0).
///
/// Needed by generic code (for example, folding operations in tests).
/// It is **not** in general the least element of the lattice, because
/// `A::default()`/`T::default()` need not be minima.
impl<A: Actor + Default, T: Ord + Clone + Default> Default for LwwRegister<A, T> {
    fn default() -> Self {
        Self::new(A::default(), T::default())
    }
}

impl<A: Actor, T: Ord + Clone> Update for LwwRegister<A, T> {
    /// Operation: triple "timestamp, actor, value".
    type Op = (u64, A, T);

    fn apply(&mut self, (timestamp, actor, value): Self::Op) {
        self.write(timestamp, actor, value);
    }
}

impl<A: Actor, T: Ord + Clone> JoinSemilattice for LwwRegister<A, T> {
    /// Maximum in the lexicographic order `(timestamp, actor, value)`.
    fn join(&mut self, other: &Self) {
        if self.key() < other.key() {
            self.clone_from(other);
        }
    }
}

/// Violation of the LWW invariant: the pair `(timestamp, actor)` was
/// reused for different values.
///
/// The LWW protocol assumes write stamps are unique (an actor never
/// issues the same `timestamp` twice). When that is broken, `join`
/// remains a correct semilattice operation, but the "winner" is chosen
/// by the fallback comparison of values, i.e. arbitrarily from the
/// point of view of the intended semantics.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MergeError<A: Actor, T: Ord + Clone> {
    /// Reused timestamp.
    pub timestamp: u64,
    /// Actor that issued the stamp twice.
    pub actor: A,
    /// Conflicting values of the two states.
    pub values: (T, T),
}

impl<A: Actor + fmt::Debug, T: Ord + Clone + fmt::Debug> fmt::Display for MergeError<A, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "stamp (timestamp {}, actor {:?}) reused: values {:?} and {:?}",
            self.timestamp, self.actor, self.values.0, self.values.1
        )
    }
}

impl<A: Actor + fmt::Debug, T: Ord + Clone + fmt::Debug> std::error::Error for MergeError<A, T> {}

impl<A: Actor, T: Ord + Clone> ValidateMerge for LwwRegister<A, T> {
    type Error = MergeError<A, T>;

    /// Error if both states carry the same pair `(timestamp, actor)`
    /// with different values.
    fn validate_merge(&self, other: &Self) -> Result<(), Self::Error> {
        if self.timestamp == other.timestamp
            && self.actor == other.actor
            && self.value != other.value
        {
            return Err(MergeError {
                timestamp: self.timestamp,
                actor: self.actor.clone(),
                values: (self.value.clone(), other.value.clone()),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_timestamp_wins() {
        let mut r = LwwRegister::new("a", 0);
        r.write(5, "a", 42);
        r.write(3, "b", 7); // stale write is ignored
        assert_eq!(*r.value(), 42);
    }

    #[test]
    fn actor_breaks_timestamp_tie() {
        let mut a = LwwRegister::new("a", 0);
        let mut b = a.clone();
        a.write(1, "a", 10);
        b.write(1, "b", 20);
        let m = a.clone().joined(&b);
        assert_eq!(m, b.joined(&a));
        assert_eq!(*m.value(), 20); // "b" > "a"
    }
}
