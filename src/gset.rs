//! G-Set — растущее (grow-only) множество.

use std::collections::BTreeSet;

use crate::lattice::JoinSemilattice;

/// Растущее множество: поддерживает только вставку, `join` — объединение.
///
/// Простейший CRDT: булеан `2^T` с операцией `∪` — полурешётка «из
/// учебника». Служит базовым случаем для property-based тестов и
/// строительным блоком составных типов (см. [`OrSet`](crate::OrSet)).
///
/// # Пример
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
    /// Пустое множество — наименьший элемент полурешётки.
    pub fn new() -> Self {
        Self {
            items: BTreeSet::new(),
        }
    }

    /// Вставить элемент. Инфляционное обновление.
    pub fn insert(&mut self, item: T) {
        self.items.insert(item);
    }

    /// Принадлежит ли элемент множеству.
    pub fn contains(&self, item: &T) -> bool {
        self.items.contains(item)
    }

    /// Число элементов.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Пусто ли множество.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Итератор по элементам в возрастающем порядке.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }
}

impl<T: Ord + Clone> JoinSemilattice for GSet<T> {
    /// Объединение множеств.
    fn join(&mut self, other: &Self) {
        self.items.extend(other.items.iter().cloned());
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
