//! G-Set: a grow-only set.

use std::collections::BTreeSet;

use crate::lattice::{JoinSemilattice, Update};

/// Grow-only set: insertion only, `join` is union.
///
/// The simplest CRDT: the power set `2^T` with `∪` is a textbook
/// semilattice. It is the base case for property-based tests and a
/// building block of composite types (see [`OrSet`](crate::OrSet)).
///
/// # Example
///
/// ```
/// use crdt_lab::{GSet, CvRdt};
///
/// let mut a = GSet::new();
/// let mut b = GSet::new();
/// a.insert(1);
/// b.insert(2);
/// a.merge(&b);
/// assert!(a.contains(&1) && a.contains(&2));
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct GSet<T: Ord + Clone> {
    items: BTreeSet<T>,
}

impl<T: Ord + Clone> GSet<T> {
    /// Empty set: the least element of the semilattice.
    pub fn new() -> Self {
        Self {
            items: BTreeSet::new(),
        }
    }

    /// Insert an element. Inflationary update.
    pub fn insert(&mut self, item: T) {
        self.items.insert(item);
    }

    /// Whether the element belongs to the set.
    pub fn contains(&self, item: &T) -> bool {
        self.items.contains(item)
    }

    /// Number of elements.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Iterator over elements in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }
}

impl<T: Ord + Clone> JoinSemilattice for GSet<T> {
    /// Set union.
    fn join(&mut self, other: &Self) {
        self.items.extend(other.items.iter().cloned());
    }
}

impl<T: Ord + Clone> Update for GSet<T> {
    /// Operation: the element being inserted.
    type Op = T;

    fn apply(&mut self, item: T) {
        self.insert(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_merge() {
        let mut a = GSet::new();
        let mut b = GSet::new();
        a.insert("x");
        b.insert("y");
        let m = a.clone().joined(&b);
        assert_eq!(m.len(), 2);
        assert!(a.leq(&m) && b.leq(&m));
    }
}
