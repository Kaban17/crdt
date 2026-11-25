//! OR-Set: observed-remove set with add-wins semantics.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::lattice::{Actor, JoinSemilattice, Update, ValidateMerge};

/// Operation on an [`OrSet`] (and [`Orswot`](crate::Orswot): a shared
/// alphabet of operations lets the two implementations be compared on
/// the same histories).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Op<A: Actor, T> {
    /// Insert an element on behalf of an actor.
    Add(A, T),
    /// Remove all observed occurrences of the element.
    Remove(T),
}

/// Unique tag of an insertion event: pair `(actor, sequence number)`.
///
/// The newtype keeps the tag from being mixed up with an ordinary pair
/// and centralizes the uniqueness invariant: fresh tags are issued only
/// by [`OrSet::add`], which increments the actor's personal counter.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Dot<A: Actor> {
    actor: A,
    counter: u64,
}

impl<A: Actor> Dot<A> {
    /// Constructor for the op-based variant ([`crate::cmrdt::OpOrSet`]):
    /// there, freshness of the tag is ensured by the source's `prepare`
    /// phase, not by this module. Deliberately `pub(crate)`: outside the
    /// crate, tags still cannot be minted by hand.
    pub(crate) fn new(actor: A, counter: u64) -> Self {
        Self { actor, counter }
    }

    /// Actor that issued the tag.
    pub fn actor(&self) -> &A {
        &self.actor
    }

    /// Sequence number of the tag for that actor.
    pub fn counter(&self) -> u64 {
        self.counter
    }
}

/// Set with deletion and **add-wins** semantics: under concurrent
/// `add(x)` and `remove(x)` the element remains.
///
/// A naive "set with deletion" is not a semilattice: deletion is not
/// inflationary. OR-Set solves this by turning both operations into
/// growth of the state:
///
/// * `add(x)` tags the insertion with a fresh unique [`Dot`];
/// * `remove(x)` does not touch `x`; it adds the **observed** insertion
///   tags of `x` to a tombstone set;
/// * the element is present if it has a tag outside the tombstones.
///
/// All three state components (insertion tags, tombstones, actor
/// counters) are growing structures; their product is again a
/// semilattice. Add-wins follows from the definition: a concurrent
/// insertion carries a tag that the deletion did not observe and
/// therefore did not bury.
///
/// The cost is unbounded growth of tombstones; the optimized variant
/// with a causal context instead of tombstones is
/// [`Orswot`](crate::Orswot).
///
/// # Example
///
/// ```
/// use crdt_lab::{OrSet, CvRdt};
///
/// let mut a: OrSet<&str, &str> = OrSet::new();
/// a.add("a", "x");
/// let mut b = a.clone();
///
/// b.remove(&"x");   // b removes the observed "x"
/// a.add("a", "x");  // a concurrently inserts "x" again
///
/// a.merge(&b);
/// assert!(a.contains(&"x")); // add-wins
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct OrSet<A: Actor, T: Ord + Clone> {
    /// Live insertion tags of each element.
    entries: BTreeMap<T, BTreeSet<Dot<A>>>,
    /// Tombstones: tags whose deletion has been observed.
    tombstones: BTreeSet<Dot<A>>,
    /// Per-actor counters used to issue fresh tags.
    counters: BTreeMap<A, u64>,
}

impl<A: Actor, T: Ord + Clone> OrSet<A, T> {
    /// Empty set.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            tombstones: BTreeSet::new(),
            counters: BTreeMap::new(),
        }
    }

    /// Insert an element on behalf of an actor, tagged with a fresh dot.
    pub fn add(&mut self, actor: A, item: T) {
        let counter = self.counters.entry(actor.clone()).or_insert(0);
        *counter += 1;
        let dot = Dot {
            actor,
            counter: *counter,
        };
        self.entries.entry(item).or_default().insert(dot);
    }

    /// Remove an element: bury all *observed* tags of its insertions.
    ///
    /// Concurrent (not yet delivered) insertions are not affected,
    /// which is where add-wins comes from.
    pub fn remove(&mut self, item: &T) {
        if let Some(dots) = self.entries.get(item) {
            self.tombstones.extend(dots.iter().cloned());
        }
    }

    /// Whether the element is present: whether it has a tag outside
    /// the tombstones.
    pub fn contains(&self, item: &T) -> bool {
        self.entries
            .get(item)
            .is_some_and(|dots| !dots.is_subset(&self.tombstones))
    }

    /// Iterator over present elements in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.entries
            .iter()
            .filter(|(_, dots)| !dots.is_subset(&self.tombstones))
            .map(|(item, _)| item)
    }

    /// Number of present elements.
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    /// Whether the set is empty (from an observer's point of view).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Number of tombstones: accumulated tags of deleted insertions.
    /// Observable metric of unbounded state growth.
    pub fn tombstone_count(&self) -> usize {
        self.tombstones.len()
    }
}

impl<A: Actor, T: Ord + Clone> JoinSemilattice for OrSet<A, T> {
    /// Componentwise `join`: union of tags and tombstones,
    /// componentwise maximum of counters.
    fn join(&mut self, other: &Self) {
        for (item, dots) in &other.entries {
            self.entries
                .entry(item.clone())
                .or_default()
                .extend(dots.iter().cloned());
        }
        self.tombstones.extend(other.tombstones.iter().cloned());
        for (actor, &n) in &other.counters {
            let e = self.counters.entry(actor.clone()).or_insert(0);
            *e = (*e).max(n);
        }
    }
}

/// Violation of the OR-Set invariant: one [`Dot`] is attached to
/// different elements in two states.
///
/// Uniqueness of tags is guaranteed by the generator ([`OrSet::add`])
/// only within a replica; a collision means one actor was used on two
/// replicas at once (or the actor's counter was reset). After merge
/// such a tag "glues" the fate of two independent insertions: one
/// `remove` will bury both.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MergeError<A: Actor, T: Ord + Clone> {
    /// Tag that appeared on different elements.
    pub dot: Dot<A>,
    /// Elements the tag is attached to in `self` and `other`.
    pub items: (T, T),
}

impl<A: Actor + fmt::Debug, T: Ord + Clone + fmt::Debug> fmt::Display for MergeError<A, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "tag {:?} is attached to different elements: {:?} and {:?}",
            self.dot, self.items.0, self.items.1
        )
    }
}

impl<A: Actor + fmt::Debug, T: Ord + Clone + fmt::Debug> std::error::Error for MergeError<A, T> {}

impl<A: Actor, T: Ord + Clone> ValidateMerge for OrSet<A, T> {
    type Error = MergeError<A, T>;

    /// Error if some insertion tag is attached to different elements
    /// in the two states.
    fn validate_merge(&self, other: &Self) -> Result<(), Self::Error> {
        let their_dots: BTreeMap<&Dot<A>, &T> = other
            .entries
            .iter()
            .flat_map(|(item, dots)| dots.iter().map(move |d| (d, item)))
            .collect();
        for (item, dots) in &self.entries {
            for dot in dots {
                if let Some(&their_item) = their_dots.get(dot) {
                    if their_item != item {
                        return Err(MergeError {
                            dot: dot.clone(),
                            items: (item.clone(), their_item.clone()),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

impl<A: Actor, T: Ord + Clone> Update for OrSet<A, T> {
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

    #[test]
    fn add_then_remove_locally() {
        let mut s: OrSet<&str, i32> = OrSet::new();
        s.add("a", 1);
        assert!(s.contains(&1));
        s.remove(&1);
        assert!(!s.contains(&1));
    }

    #[test]
    fn concurrent_add_wins_over_remove() {
        let mut a: OrSet<&str, i32> = OrSet::new();
        a.add("a", 1);
        let mut b = a.clone();

        b.remove(&1);
        a.add("a", 1); // concurrent re-insertion

        let m = a.joined(&b);
        assert!(m.contains(&1));
    }

    #[test]
    fn remove_only_observed_dots() {
        let mut a: OrSet<&str, i32> = OrSet::new();
        let mut b: OrSet<&str, i32> = OrSet::new();
        b.add("b", 1);
        // a has not seen the insertion, so its remove buries nothing
        a.remove(&1);
        let m = a.joined(&b);
        assert!(m.contains(&1));
    }

    #[test]
    fn readd_after_merge_of_remove() {
        let mut a: OrSet<&str, i32> = OrSet::new();
        a.add("a", 1);
        a.remove(&1);
        assert!(!a.contains(&1));
        a.add("a", 1); // the fresh tag is not buried
        assert!(a.contains(&1));
    }
}
