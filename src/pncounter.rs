//! PN-Counter: a counter with increment and decrement.

use crate::gcounter::GCounter;
use crate::lattice::{Actor, JoinSemilattice, Update};

/// Operation on a [`PnCounter`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Op<A: Actor> {
    /// Increment the counter by 1 on behalf of an actor.
    Inc(A),
    /// Decrement the counter by 1 on behalf of an actor.
    Dec(A),
}

/// Counter with both increment *and* decrement.
///
/// Decrement is not an inflationary operation on an ordinary number, so
/// the type is built as a **product of two semilattices**: a pair of
/// grow-only counters of "pluses" and "minuses". A product of
/// semilattices with componentwise `join` is again a semilattice. The
/// library uses that composition property directly: `join` of the pair
/// delegates to `join` of the components, and the laws are inherited
/// by construction.
///
/// The observed value is the difference `plus - minus` (computed in
/// `i64`).
///
/// # Example
///
/// ```
/// use crdt_lab::{PnCounter, CvRdt};
///
/// let mut a = PnCounter::new();
/// let mut b = PnCounter::new();
/// a.increment("a");
/// a.increment("a");
/// b.decrement("b");
///
/// a.merge(&b);
/// assert_eq!(a.value(), 1); // 2 - 1
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PnCounter<A: Actor> {
    plus: GCounter<A>,
    minus: GCounter<A>,
}

impl<A: Actor> PnCounter<A> {
    /// Zero counter.
    pub fn new() -> Self {
        Self {
            plus: GCounter::new(),
            minus: GCounter::new(),
        }
    }

    /// Increment the counter by 1 on behalf of an actor.
    pub fn increment(&mut self, actor: A) {
        self.plus.increment(actor);
    }

    /// Decrement the counter by 1 on behalf of an actor.
    ///
    /// In the lattice of states the operation is still *inflationary*:
    /// the `minus` component grows, even though the observed value
    /// decreases. The split between "order of states" and "observed
    /// value" is what makes decrement work.
    pub fn decrement(&mut self, actor: A) {
        self.minus.increment(actor);
    }

    /// Current value: sum of pluses minus sum of minuses.
    pub fn value(&self) -> i64 {
        self.plus.value() as i64 - self.minus.value() as i64
    }

    /// Increment component (for inspecting the structure of the state).
    pub fn plus(&self) -> &GCounter<A> {
        &self.plus
    }

    /// Decrement component (for inspecting the structure of the state).
    pub fn minus(&self) -> &GCounter<A> {
        &self.minus
    }
}

impl<A: Actor> JoinSemilattice for PnCounter<A> {
    /// Componentwise `join` of the product of semilattices.
    fn join(&mut self, other: &Self) {
        self.plus.join(&other.plus);
        self.minus.join(&other.minus);
    }
}

impl<A: Actor> Update for PnCounter<A> {
    type Op = Op<A>;

    fn apply(&mut self, op: Self::Op) {
        match op {
            Op::Inc(actor) => self.increment(actor),
            Op::Dec(actor) => self.decrement(actor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrement_inflates_state_but_lowers_value() {
        let mut c = PnCounter::new();
        c.increment("a");
        let before = c.clone();
        c.decrement("a");
        assert_eq!(c.value(), 0);
        assert!(before.leq(&c)); // the state grew
    }

    #[test]
    fn concurrent_inc_dec_both_survive_merge() {
        let mut a = PnCounter::new();
        let mut b = a.clone();
        a.increment("a");
        b.decrement("b");
        let m = a.joined(&b);
        assert_eq!(m.value(), 0);
    }
}
