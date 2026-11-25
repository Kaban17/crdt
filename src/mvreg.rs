//! MV-Register: a multi-value register.

use std::fmt;

use crate::lattice::{Actor, JoinSemilattice, Update, ValidateMerge};
use crate::vclock::VClock;

/// Register entry: a value with a causal context.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Entry<A: Actor, T> {
    clock: VClock<A>,
    value: T,
}

/// Multi-value register: concurrent writes are **not lost**.
///
/// Counterpart of [`LwwRegister`](crate::LwwRegister): instead of
/// picking a single deterministic "winner", the register keeps *all*
/// causally incomparable values and hands them to the reader for
/// resolution (as Dynamo does with sibling values).
///
/// State is an **antichain** of pairs `(vector clock, value)`: no clock
/// strictly dominates another. A write observes all current clocks
/// (takes their `⊔` and increments its own component), so it displaces
/// everything it has seen; merge keeps the maximal elements of the
/// union, so concurrent writes both survive.
///
/// For a canonical representation (needed by `Eq` of the lattice laws)
/// entries are stored sorted by an arbitrary total order on clocks and
/// deduplicated.
///
/// # Example
///
/// ```
/// use crdt_lab::{MvRegister, CvRdt};
///
/// let mut a = MvRegister::new();
/// let mut b = a.clone();
/// a.write("a", 1);
/// b.write("b", 2);
///
/// a.merge(&b);
/// let mut seen: Vec<i32> = a.read().copied().collect();
/// seen.sort();
/// assert_eq!(seen, [1, 2]); // both concurrent values are kept
///
/// a.write("a", 3); // this write observed both values…
/// assert_eq!(a.read().copied().collect::<Vec<_>>(), [3]); // …and displaced them
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MvRegister<A: Actor, T: Ord + Clone> {
    entries: Vec<Entry<A, T>>,
}

impl<A: Actor, T: Ord + Clone> Default for MvRegister<A, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Actor, T: Ord + Clone> MvRegister<A, T> {
    /// Empty register: the least element of the semilattice.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Current values: all causally incomparable entries.
    pub fn read(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().map(|e| &e.value)
    }

    /// Entries together with their causal context (for inspecting state).
    pub fn read_with_clocks(&self) -> impl Iterator<Item = (&VClock<A>, &T)> {
        self.entries.iter().map(|e| (&e.clock, &e.value))
    }

    /// Number of stored concurrent values.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the register is empty (no writes yet).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Write a value on behalf of an actor.
    ///
    /// The clock of the new entry is the `⊔` of all observed clocks
    /// plus the actor's own event, so the write strictly dominates the
    /// whole current state: the operation is inflationary, and
    /// everything observed is displaced.
    pub fn write(&mut self, actor: A, value: T) {
        let mut clock = VClock::new();
        for e in &self.entries {
            clock.join(&e.clock);
        }
        clock.increment(actor);
        self.entries = vec![Entry { clock, value }];
    }

    /// Canonicalize: drop strictly dominated entries (keep an
    /// antichain), sort, and deduplicate.
    fn canonicalize(candidates: Vec<Entry<A, T>>) -> Vec<Entry<A, T>> {
        let mut kept: Vec<Entry<A, T>> = candidates
            .iter()
            .enumerate()
            .filter(|(i, e)| {
                !candidates
                    .iter()
                    .enumerate()
                    .any(|(j, o)| *i != j && e.clock < o.clock)
            })
            .map(|(_, e)| e.clone())
            .collect();
        kept.sort_by(|x, y| {
            x.clock
                .repr_cmp(&y.clock)
                .then_with(|| x.value.cmp(&y.value))
        });
        kept.dedup();
        kept
    }
}

impl<A: Actor, T: Ord + Clone> JoinSemilattice for MvRegister<A, T> {
    /// Maximal elements of the union of two antichains.
    fn join(&mut self, other: &Self) {
        let mut candidates = std::mem::take(&mut self.entries);
        candidates.extend(other.entries.iter().cloned());
        self.entries = Self::canonicalize(candidates);
    }
}

/// Violation of the MV-register invariant: the same vector clock
/// accompanies different values.
///
/// The clock of an entry uniquely identifies an event (actor + that
/// actor's event number), so the map "clock → value" must be injective.
/// A collision means an actor issued one event number for two writes:
/// after merge both remain in the antichain as "concurrent", although
/// they are causally identical.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MergeError<A: Actor, T: Ord + Clone> {
    /// Clock that appeared with different values.
    pub clock: VClock<A>,
    /// Conflicting values from `self` and `other`.
    pub values: (T, T),
}

impl<A: Actor + fmt::Debug, T: Ord + Clone + fmt::Debug> fmt::Display for MergeError<A, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "clock {:?} accompanies different values: {:?} and {:?}",
            self.clock, self.values.0, self.values.1
        )
    }
}

impl<A: Actor + fmt::Debug, T: Ord + Clone + fmt::Debug> std::error::Error for MergeError<A, T> {}

impl<A: Actor, T: Ord + Clone> ValidateMerge for MvRegister<A, T> {
    type Error = MergeError<A, T>;

    /// Error if the two states contain entries with equal clocks but
    /// different values.
    fn validate_merge(&self, other: &Self) -> Result<(), Self::Error> {
        for e in &self.entries {
            for o in &other.entries {
                if e.clock == o.clock && e.value != o.value {
                    return Err(MergeError {
                        clock: e.clock.clone(),
                        values: (e.value.clone(), o.value.clone()),
                    });
                }
            }
        }
        Ok(())
    }
}

impl<A: Actor, T: Ord + Clone> Update for MvRegister<A, T> {
    /// Operation: pair "actor, value".
    type Op = (A, T);

    fn apply(&mut self, (actor, value): Self::Op) {
        self.write(actor, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted_read(reg: &MvRegister<&str, i32>) -> Vec<i32> {
        let mut values: Vec<i32> = reg.read().copied().collect();
        values.sort();
        values
    }

    #[test]
    fn sequential_writes_keep_last() {
        let mut r = MvRegister::new();
        r.write("a", 1);
        r.write("a", 2);
        r.write("b", 3);
        assert_eq!(sorted_read(&r), [3]);
    }

    #[test]
    fn concurrent_writes_both_survive() {
        let mut a = MvRegister::new();
        let mut b = a.clone();
        a.write("a", 1);
        b.write("b", 2);
        let merged = a.joined(&b);
        assert_eq!(sorted_read(&merged), [1, 2]);
    }

    #[test]
    fn observed_write_displaces_merged_values() {
        let mut a = MvRegister::new();
        let mut b = a.clone();
        a.write("a", 1);
        b.write("b", 2);
        a.join(&b);
        a.write("a", 3);
        assert_eq!(sorted_read(&a), [3]);
        // and this state dominates the earlier ones
        assert!(b.leq(&a));
    }

    #[test]
    fn merge_after_observed_write_is_stable() {
        let mut a = MvRegister::new();
        let mut b = a.clone();
        a.write("a", 1);
        b.write("b", 2);
        a.join(&b);
        a.write("a", 3);
        // redelivering the old state does not resurrect anything
        a.join(&b);
        assert_eq!(sorted_read(&a), [3]);
    }
}
