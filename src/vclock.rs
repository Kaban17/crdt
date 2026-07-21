//! Векторные часы — каноническая полурешётка и строительный блок CRDT.

use std::collections::BTreeMap;

use crate::lattice::{Actor, JoinSemilattice, Update};

/// Векторные часы: отображение `актор → счётчик событий`.
///
/// Полурешётка с покомпонентным максимумом. Отсутствующая компонента
/// считается равной нулю, поэтому пустые часы — наименьший элемент (`⊥`).
///
/// Помимо [`JoinSemilattice`], тип реализует [`PartialOrd`]: два вектора
/// **несравнимы** тогда и только тогда, когда соответствующие события
/// конкурентны (ни одно не «произошло-до» другого, отношение Лампорта).
/// Это пример того, как частичный порядок теории напрямую отображается
/// в стандартный трейт Rust.
///
/// # Пример
///
/// ```
/// use crdt_lab::{VClock, JoinSemilattice};
///
/// let mut a = VClock::new();
/// let mut b = VClock::new();
/// a.increment("a");
/// b.increment("b");
///
/// // события конкурентны: векторы несравнимы
/// assert_eq!(a.partial_cmp(&b), None);
///
/// // join — точная верхняя грань
/// let ab = a.clone().joined(&b);
/// assert!(a.leq(&ab) && b.leq(&ab));
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct VClock<A: Actor> {
    entries: BTreeMap<A, u64>,
}

impl<A: Actor> VClock<A> {
    /// Пустые часы — наименьший элемент полурешётки.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Счётчик событий актора (0, если актор ещё не наблюдался).
    pub fn get(&self, actor: &A) -> u64 {
        self.entries.get(actor).copied().unwrap_or(0)
    }

    /// Зарегистрировать событие актора. Инфляционно: `self ≤ self'`.
    pub fn increment(&mut self, actor: A) {
        *self.entries.entry(actor).or_insert(0) += 1;
    }

    /// Покомпонентное сравнение `self ≤ other`.
    fn dominated_by(&self, other: &Self) -> bool {
        self.entries.iter().all(|(a, &n)| n <= other.get(a))
    }
}

impl<A: Actor> JoinSemilattice for VClock<A> {
    /// Покомпонентный максимум.
    fn join(&mut self, other: &Self) {
        for (actor, &n) in &other.entries {
            let e = self.entries.entry(actor.clone()).or_insert(0);
            *e = (*e).max(n);
        }
    }
}

impl<A: Actor> Update for VClock<A> {
    /// Операция — идентификатор актора, зарегистрировавшего событие.
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
            (false, false) => None, // конкурентные события
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

    /// Выведенный из `join` порядок совпадает с «произошло-до» из
    /// `PartialOrd` — согласованность двух определений.
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
