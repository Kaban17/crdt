//! ORSWOT: observed-remove set without tombstones.

use std::collections::{BTreeMap, BTreeSet};

use crate::lattice::{Actor, JoinSemilattice, Update};
use crate::orset::{Dot, Op};
use crate::vclock::VClock;

/// Optimized OR-Set: a causal context instead of tombstones
/// (Bieniusa et al., 2012; Riak's implementation).
///
/// In the state-based [`OrSet`](crate::OrSet), deletion adds a tag to a
/// forever-growing tombstone set. ORSWOT replaces tombstones with a
/// **causal context**: a vector clock of every tag ever *seen*:
///
/// * `add` issues a fresh tag and advances the context;
/// * `remove` **physically** deletes the element's tags; the context
///   remembers that they were seen;
/// * absence of a tag in the state has two meanings, distinguished by
///   the context: "not yet seen" (tag outside the context) or "seen
///   and deleted" (tag covered by the context).
///
/// Therefore `join` is **not** componentwise union: a tag survives if
/// it is in both states, or in one and *not covered* by the other's
/// context (the other has not seen it yet: a concurrent insertion,
/// add-wins). A tag present in one state but covered by the other's
/// context was deleted there, and dies on merge.
///
/// `remove` *shrinks* the state structurally, yet remains an
/// **inflationary** operation of the derived order
/// (`a ≤ b ⟺ a ⊔ b = b`): the semilattice order measures knowledge,
/// not size. "Seen and deleted" is strictly greater than "seen".
/// Redelivering an old state therefore resurrects nothing.
///
/// State size is bounded: live tags plus one counter per actor. The
/// trace of add/remove cycles does not accumulate (see the test
/// `state_stays_bounded_after_add_remove_cycles`).
///
/// # Example
///
/// ```
/// use crdt_lab::{Orswot, CvRdt};
///
/// let mut a: Orswot<&str, &str> = Orswot::new();
/// a.add("a", "x");
/// let mut b = a.clone();
///
/// b.remove(&"x");   // b physically deletes the observed "x"
/// a.add("a", "x");  // a concurrently inserts "x" again
///
/// a.merge(&b);
/// assert!(a.contains(&"x")); // add-wins: the fresh tag is not covered by b's context
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Orswot<A: Actor, T: Ord + Clone> {
    /// Live insertion tags of each element (no empty entries).
    entries: BTreeMap<T, BTreeSet<Dot<A>>>,
    /// Causal context: every tag of each actor ever seen.
    context: VClock<A>,
}

impl<A: Actor, T: Ord + Clone> Orswot<A, T> {
    /// Empty set.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            context: VClock::new(),
        }
    }

    /// Insert an element on behalf of an actor, tagged with a fresh dot.
    pub fn add(&mut self, actor: A, item: T) {
        let next = self.context.get(&actor) + 1;
        self.context.increment(actor.clone());
        self.entries
            .entry(item)
            .or_default()
            .insert(Dot::new(actor, next));
    }

    /// Remove an element: physically erase its tags.
    ///
    /// The context does not change: it already covers the erased tags,
    /// and that is what distinguishes "deleted" from "not seen" on merge.
    pub fn remove(&mut self, item: &T) {
        self.entries.remove(item);
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

    /// Causal context (for inspecting state size).
    pub fn context(&self) -> &VClock<A> {
        &self.context
    }

    /// Number of live tags. Together with the context this is the
    /// whole size of the state (compare
    /// [`OrSet::tombstone_count`](crate::OrSet::tombstone_count)).
    pub fn dot_count(&self) -> usize {
        self.entries.values().map(BTreeSet::len).sum()
    }

    /// Whether tag `dot` of element `item` from state `src` survives
    /// a merge with state `dst`.
    fn survives(dot: &Dot<A>, item: &T, dst: &Self) -> bool {
        dst.entries.get(item).is_some_and(|dots| dots.contains(dot))
            || dot.counter() > dst.context.get(dot.actor())
    }
}

impl<A: Actor, T: Ord + Clone> JoinSemilattice for Orswot<A, T> {
    /// Merge by the rule "live in both, or live in one and unseen by
    /// the other"; contexts are joined by componentwise maximum.
    fn join(&mut self, other: &Self) {
        let mut merged: BTreeMap<T, BTreeSet<Dot<A>>> = BTreeMap::new();
        for (item, dots) in &self.entries {
            let kept: BTreeSet<Dot<A>> = dots
                .iter()
                .filter(|dot| Self::survives(dot, item, other))
                .cloned()
                .collect();
            if !kept.is_empty() {
                merged.insert(item.clone(), kept);
            }
        }
        for (item, dots) in &other.entries {
            for dot in dots.iter().filter(|dot| Self::survives(dot, item, self)) {
                merged.entry(item.clone()).or_default().insert(dot.clone());
            }
        }
        self.entries = merged;
        self.context.join(&other.context);
    }
}

impl<A: Actor, T: Ord + Clone> Update for Orswot<A, T> {
    /// The same alphabet of operations as [`OrSet`](crate::OrSet):
    /// shared test strategies and a direct comparison of the two
    /// implementations.
    type Op = Op<A, T>;

    fn apply(&mut self, op: Self::Op) {
        match op {
            Op::Add(actor, item) => self.add(actor, item),
            Op::Remove(item) => self.remove(&item),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OrSet;

    #[test]
    fn add_then_remove_locally() {
        let mut s: Orswot<&str, i32> = Orswot::new();
        s.add("a", 1);
        assert!(s.contains(&1));
        s.remove(&1);
        assert!(!s.contains(&1));
        assert!(s.is_empty());
    }

    #[test]
    fn concurrent_add_wins_over_remove() {
        let mut a: Orswot<&str, i32> = Orswot::new();
        a.add("a", 1);
        let mut b = a.clone();

        b.remove(&1);
        a.add("a", 1); // concurrent re-insertion

        let m = a.joined(&b);
        assert!(m.contains(&1));
    }

    #[test]
    fn remove_of_unseen_element_loses_to_add() {
        let mut a: Orswot<&str, i32> = Orswot::new();
        let mut b: Orswot<&str, i32> = Orswot::new();
        b.add("b", 1);
        a.remove(&1); // a has not seen the insertion, so there is nothing to erase
        let m = a.joined(&b);
        assert!(m.contains(&1));
    }

    #[test]
    fn observed_remove_kills_dot_on_merge() {
        let mut a: Orswot<&str, i32> = Orswot::new();
        a.add("a", 1);
        let stale = a.clone(); // old state with a live tag
        a.remove(&1);
        // redelivering the old state does not resurrect the element:
        // its tag is covered by a's context
        let m = a.joined(&stale);
        assert!(!m.contains(&1));
    }

    /// Structurally, remove shrinks the state, but in the derived
    /// semilattice order the state grows: the order measures
    /// knowledge, not size.
    #[test]
    fn remove_is_inflationary_in_derived_order() {
        let mut s: Orswot<&str, i32> = Orswot::new();
        s.add("a", 1);
        let before = s.clone();
        s.remove(&1);
        assert!(before.leq(&s));
        assert!(!s.leq(&before));
    }

    /// The central difference from a tombstone OR-Set: the history
    /// trace does not accumulate; state size is bounded by live tags
    /// plus one counter per actor.
    #[test]
    fn state_stays_bounded_after_add_remove_cycles() {
        let mut swot: Orswot<&str, i32> = Orswot::new();
        let mut orset: OrSet<&str, i32> = OrSet::new();
        for _ in 0..100 {
            swot.add("a", 1);
            swot.remove(&1);
            orset.add("a", 1);
            orset.remove(&1);
        }
        // the tombstone OR-Set accumulated a trace of every insertion…
        assert_eq!(orset.tombstone_count(), 100);
        // …ORSWOT has zero live tags and one context counter
        assert_eq!(swot.dot_count(), 0);
        assert_eq!(swot.context().get(&"a"), 100);
        assert!(swot.is_empty());
    }
}
