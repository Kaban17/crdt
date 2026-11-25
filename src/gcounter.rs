//! G-Counter: a grow-only distributed counter.

use std::collections::BTreeMap;

use crate::lattice::{Actor, JoinSemilattice, Update};

/// Grow-only counter: each replica increments only *its own* component;
/// the observed value is the sum of all components.
///
/// State is a map `actor → personal contribution`; the semilattice is
/// the same as for vector clocks (componentwise maximum). Correctness
/// of merge rests on the invariant "actor `a`'s component is increased
/// only by replica `a`", so that component on any other replica is a
/// stale (smaller or equal) value of the true contribution, and `max`
/// restores it.
///
/// # Example
///
/// ```
/// use crdt_lab::{GCounter, CvRdt};
///
/// let mut a = GCounter::new();
/// let mut b = GCounter::new();
/// a.increment("a");
/// b.increment("b");
/// b.increment("b");
///
/// a.merge(&b);
/// b.merge(&a);
/// assert_eq!(a, b);          // convergence
/// assert_eq!(a.value(), 3);  // 1 + 2
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct GCounter<A: Actor> {
    counts: BTreeMap<A, u64>,
}

impl<A: Actor> GCounter<A> {
    /// Zero counter: the least element of the semilattice.
    pub fn new() -> Self {
        Self {
            counts: BTreeMap::new(),
        }
    }

    /// Increase the actor's contribution by 1. Inflationary update.
    pub fn increment(&mut self, actor: A) {
        self.increment_by(actor, 1);
    }

    /// Increase the actor's contribution by `amount`. Inflationary update.
    pub fn increment_by(&mut self, actor: A, amount: u64) {
        *self.counts.entry(actor).or_insert(0) += amount;
    }

    /// Current counter value: sum of all actors' contributions.
    pub fn value(&self) -> u64 {
        self.counts.values().sum()
    }

    /// Contribution of a single actor (0 if the actor has not been observed).
    pub fn get(&self, actor: &A) -> u64 {
        self.counts.get(actor).copied().unwrap_or(0)
    }
}

impl<A: Actor> JoinSemilattice for GCounter<A> {
    /// Componentwise maximum of contributions.
    fn join(&mut self, other: &Self) {
        for (actor, &n) in &other.counts {
            let e = self.counts.entry(actor.clone()).or_insert(0);
            *e = (*e).max(n);
        }
    }
}

impl<A: Actor> Update for GCounter<A> {
    /// Operation: pair "actor, increment amount".
    type Op = (A, u64);

    fn apply(&mut self, (actor, amount): Self::Op) {
        self.increment_by(actor, amount);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_does_not_double_count() {
        let mut a = GCounter::new();
        a.increment("a");
        let b = a.clone();
        // redelivering the same state does not change the value
        let merged = a.clone().joined(&b).joined(&b);
        assert_eq!(merged.value(), 1);
    }

    #[test]
    fn increment_is_inflationary() {
        let mut c = GCounter::new();
        let before = c.clone();
        c.increment("a");
        assert!(before.leq(&c));
        assert!(!c.leq(&before));
    }
}
