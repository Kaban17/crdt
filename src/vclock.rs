//! Vector clocks: the canonical semilattice and a building block of CRDTs.

use std::collections::BTreeMap;

use crate::lattice::{Actor, JoinSemilattice, Update};

/// Vector clock: a map `actor → event count`.
///
/// A semilattice under componentwise maximum. A missing component is
/// treated as zero, so the empty clock is the least element (`⊥`).
///
/// Besides [`JoinSemilattice`], the type implements [`PartialOrd`]: two
/// vectors are **incomparable** if and only if the corresponding events
/// are concurrent (neither happened-before the other, Lamport's
/// relation). The theoretical partial order maps directly onto a
/// standard Rust trait.
///
/// # Example
///
/// ```
/// use crdt_lab::{VClock, JoinSemilattice};
///
/// let mut a = VClock::new();
/// let mut b = VClock::new();
/// a.increment("a");
/// b.increment("b");
///
/// // concurrent events: the vectors are incomparable
/// assert_eq!(a.partial_cmp(&b), None);
///
/// // join is the least upper bound
/// let ab = a.clone().joined(&b);
/// assert!(a.leq(&ab) && b.leq(&ab));
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct VClock<A: Actor> {
    entries: BTreeMap<A, u64>,
}

impl<A: Actor> VClock<A> {
    /// Empty clock: the least element of the semilattice.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Event count of an actor (0 if the actor has not been observed).
    pub fn get(&self, actor: &A) -> u64 {
        self.entries.get(actor).copied().unwrap_or(0)
    }

    /// Record an event of `actor`. Inflationary: `self ≤ self'`.
    pub fn increment(&mut self, actor: A) {
        *self.entries.entry(actor).or_insert(0) += 1;
    }

    /// Componentwise comparison `self ≤ other`.
    fn dominated_by(&self, other: &Self) -> bool {
        self.entries.iter().all(|(a, &n)| n <= other.get(a))
    }

    /// An arbitrary **total** order on the representation, used to
    /// canonicalize collections of clocks (see
    /// [`MvRegister`](crate::MvRegister)).
    ///
    /// It is unrelated to the causal [`PartialOrd`] and is therefore
    /// deliberately not exposed as `Ord`: a total `Ord` that
    /// contradicts a partial `PartialOrd` would violate the standard
    /// library contract.
    pub(crate) fn repr_cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.entries.cmp(&other.entries)
    }
}

impl<A: Actor> JoinSemilattice for VClock<A> {
    /// Componentwise maximum.
    fn join(&mut self, other: &Self) {
        for (actor, &n) in &other.entries {
            let e = self.entries.entry(actor.clone()).or_insert(0);
            *e = (*e).max(n);
        }
    }
}

impl<A: Actor> Update for VClock<A> {
    /// Operation: identifier of the actor that recorded an event.
    type Op = A;

    fn apply(&mut self, actor: A) {
        self.increment(actor);
    }
}

impl<A: Actor> PartialOrd for VClock<A> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        use std::cmp::Ordering::*;
        match (self.dominated_by(other), other.dominated_by(self)) {
            (true, true) => Some(Equal),
            (true, false) => Some(Less),
            (false, true) => Some(Greater),
            (false, false) => None, // concurrent events
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happens_before_is_less() {
        let mut a = VClock::new();
        a.increment("x");
        let mut b = a.clone();
        b.increment("y");
        assert!(a < b);
        assert!(a.leq(&b));
    }

    #[test]
    fn concurrent_events_incomparable() {
        let mut a = VClock::new();
        let mut b = VClock::new();
        a.increment("x");
        b.increment("y");
        assert_eq!(a.partial_cmp(&b), None);
        assert!(!a.leq(&b) && !b.leq(&a));
    }

    /// The order derived from `join` agrees with happened-before from
    /// `PartialOrd`: the two definitions are consistent.
    #[test]
    fn derived_order_agrees_with_partial_ord() {
        let mut a = VClock::new();
        let mut b = VClock::new();
        a.increment("x");
        b.increment("x");
        b.increment("y");
        assert_eq!(a.leq(&b), a <= b);
        assert_eq!(b.leq(&a), b <= a);
    }

    #[test]
    fn join_is_least_upper_bound() {
        let mut a = VClock::new();
        let mut b = VClock::new();
        a.increment("x");
        a.increment("x");
        b.increment("x");
        b.increment("y");
        let j = a.clone().joined(&b);
        assert_eq!(j.get(&"x"), 2);
        assert_eq!(j.get(&"y"), 1);
    }
}
