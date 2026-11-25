//! Op-based model: CmRDT (commutative replicated data type).
//!
//! The second class of CRDT in Shapiro et al.: replicas exchange
//! **operations**, not states. An update is two-phase:
//!
//! 1. *prepare* runs only at the source, reads local state, and
//!    produces an operation (for example, tagging an insertion with a
//!    fresh identifier);
//! 2. *effect* is applied on **every** replica, including the source.
//!
//! Convergence requires that `effect` of concurrent operations
//! commutes, **and** that the transport delivers operations in causal
//! order and exactly once. Contrast with CvRDT:
//!
//! * the semilattice laws (ACI) are properties of the *type*, checked
//!   generically for any `T: JoinSemilattice`; idempotence of `⊔`
//!   forgives duplicates and overlapping states in the transport;
//! * the hypotheses of the op-based model are properties of the
//!   *transport*: causal delivery and exactly-once cannot be attached
//!   to the operation type by a trait; there is nowhere to write them
//!   in the signature of [`CmRdt::effect`]. That is a genuine limit of
//!   putting the specification in types (cf. Propel/VeriFx, where
//!   delivery hypotheses are part of the verified specification).
//!
//! The `prepare` phase stays as methods of concrete types
//! ([`OpOrSet::prepare_add`] and so on): its signature is
//! type-specific, and there is no shared trait for it.
//!
//! The causal-broadcast model and convergence/divergence tests live in
//! `tests/cmrdt.rs`.

use std::collections::{BTreeMap, BTreeSet};

use crate::lattice::Actor;
use crate::orset::Dot;

/// Operation-based CRDT: the *effect* phase.
///
/// # Contract (checked by tests, not by the compiler)
///
/// For operations `p`, `q` produced **concurrently** (neither observed
/// the other): `effect(effect(s, p), q) = effect(effect(s, q), p)`.
///
/// For causally ordered operations commutativity is not required, but
/// then correctness depends on the assumption that the transport
/// delivers them in causal order (see the divergence counter-test in
/// `tests/cmrdt.rs`). Unlike [`JoinSemilattice::join`] (which is
/// idempotent), `effect` is in general not idempotent: redelivery is
/// also the transport's job.
///
/// [`JoinSemilattice::join`]: crate::JoinSemilattice::join
pub trait CmRdt: Clone + Eq {
    /// Type of the transmitted operation.
    type Op: Clone;

    /// Apply a delivered operation to the state.
    fn effect(&mut self, op: &Self::Op);
}

/// Op-based counter: the simplest CmRDT.
///
/// The operation is an integer delta; addition commutes with no
/// assumptions about concurrency, so the counter converges under
/// *any* delivery order. Causal broadcast is not needed (exactly-once
/// still is: `+δ` is not idempotent). Compare with [`OpOrSet`], where
/// commutativity rests only on causal delivery.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct OpCounter {
    value: i64,
}

impl OpCounter {
    /// Zero counter.
    pub fn new() -> Self {
        Self { value: 0 }
    }

    /// Current value.
    pub fn value(&self) -> i64 {
        self.value
    }

    /// *prepare* phase: produce a delta operation.
    ///
    /// The state is not read: a degenerate case that explains why the
    /// counter does not care about delivery order.
    pub fn prepare(&self, delta: i64) -> i64 {
        delta
    }
}

impl CmRdt for OpCounter {
    type Op = i64;

    fn effect(&mut self, op: &Self::Op) {
        self.value += op;
    }
}

/// Operation on an [`OpOrSet`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Op<A: Actor, T> {
    /// Insertion: an element with a fresh tag issued by the source's
    /// `prepare` phase.
    Add {
        /// Fresh unique insertion tag.
        dot: Dot<A>,
        /// Element being inserted.
        item: T,
    },
    /// Removal: the set of tags *observed* for the element at the source.
    Remove {
        /// Insertion tags of the element observed at the source.
        dots: BTreeSet<Dot<A>>,
        /// Element being removed.
        item: T,
    },
}

/// Op-based OR-Set (add-wins).
///
/// Unlike the state-based [`OrSet`](crate::OrSet), tombstones are not
/// needed: `Remove` carries an explicit list of observed tags, and
/// `effect` deletes them physically. Add-wins is obtained the same way
/// as in the state-based variant: a concurrent insertion carries a tag
/// that the removal did not observe.
///
/// The cost is that commutativity of `effect` rests on **causal
/// delivery**: if `Remove(d)` is delivered before the `Add` that
/// produced `d`, the removal is a no-op, and then `Add` "resurrects"
/// the element. Replicas diverge forever (counter-test in
/// `tests/cmrdt.rs`). For a state-based OR-Set that situation is
/// impossible by construction: the tombstone is part of growing state,
/// and `⊔` survives any order of merges.
///
/// The model assumes the source immediately applies `effect` of a
/// prepared operation to itself (self-delivery is instant);
/// `prepare_add` without an intervening `effect` would issue a
/// duplicate tag.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct OpOrSet<A: Actor, T: Ord + Clone> {
    /// Live insertion tags of each element (no empty entries).
    entries: BTreeMap<T, BTreeSet<Dot<A>>>,
    /// Highest tag number seen for each actor: the base of the
    /// `prepare` phase when issuing fresh tags.
    counters: BTreeMap<A, u64>,
}

impl<A: Actor, T: Ord + Clone> OpOrSet<A, T> {
    /// Empty set.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            counters: BTreeMap::new(),
        }
    }

    /// *prepare* phase of insertion: tag the element with a fresh actor
    /// identifier.
    pub fn prepare_add(&self, actor: A, item: T) -> Op<A, T> {
        let next = self.counters.get(&actor).copied().unwrap_or(0) + 1;
        Op::Add {
            dot: Dot::new(actor, next),
            item,
        }
    }

    /// *prepare* phase of removal: collect the observed tags of the
    /// element.
    pub fn prepare_remove(&self, item: &T) -> Op<A, T> {
        Op::Remove {
            dots: self.entries.get(item).cloned().unwrap_or_default(),
            item: item.clone(),
        }
    }

    /// Whether the element is present.
    pub fn contains(&self, item: &T) -> bool {
        self.entries.contains_key(item)
    }

    /// Iterator over present elements in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.entries.keys()
    }

    /// Number of present elements.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<A: Actor, T: Ord + Clone> CmRdt for OpOrSet<A, T> {
    type Op = Op<A, T>;

    fn effect(&mut self, op: &Self::Op) {
        match op {
            Op::Add { dot, item } => {
                self.entries
                    .entry(item.clone())
                    .or_default()
                    .insert(dot.clone());
                let seen = self.counters.entry(dot.actor().clone()).or_insert(0);
                *seen = (*seen).max(dot.counter());
            }
            Op::Remove { dots, item } => {
                if let Some(live) = self.entries.get_mut(item) {
                    for dot in dots {
                        live.remove(dot);
                    }
                    if live.is_empty() {
                        self.entries.remove(item);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_sums_deltas() {
        let mut c = OpCounter::new();
        c.effect(&c.prepare(3));
        c.effect(&c.prepare(-1));
        assert_eq!(c.value(), 2);
    }

    #[test]
    fn add_then_remove_locally() {
        let mut s: OpOrSet<&str, i32> = OpOrSet::new();
        let add = s.prepare_add("a", 1);
        s.effect(&add);
        assert!(s.contains(&1));
        let rem = s.prepare_remove(&1);
        s.effect(&rem);
        assert!(!s.contains(&1));
        assert!(s.is_empty());
    }

    #[test]
    fn prepare_add_issues_fresh_dots() {
        let mut s: OpOrSet<&str, i32> = OpOrSet::new();
        let first = s.prepare_add("a", 1);
        s.effect(&first);
        let second = s.prepare_add("a", 1);
        assert_ne!(first, second); // tags differ; the state distinguishes them
    }
}
