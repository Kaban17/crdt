//! G-Counter — растущий (grow-only) распределённый счётчик.

use std::collections::BTreeMap;

use crate::lattice::{Actor, JoinSemilattice};

/// Растущий счётчик: каждая реплика увеличивает только *свою* компоненту,
/// итоговое значение — сумма всех компонент.
///
/// Состояние — отображение `актор → личный вклад`; полурешётка — та же,
/// что у векторных часов (покомпонентный максимум). Корректность слияния
/// опирается на инвариант «компоненту актора `a` увеличивает только сама
/// реплика `a`», поэтому её компонента на любой реплике — это устаревшее
/// (меньшее либо равное) значение истинного вклада, и `max` его
/// восстанавливает.
///
/// # Пример
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
/// assert_eq!(a, b);          // сходимость
/// assert_eq!(a.value(), 3);  // 1 + 2
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct GCounter<A: Actor> {
    counts: BTreeMap<A, u64>,
}

impl<A: Actor> GCounter<A> {
    /// Нулевой счётчик — наименьший элемент полурешётки.
    pub fn new() -> Self {
        Self {
            counts: BTreeMap::new(),
        }
    }

    /// Увеличить вклад актора на 1. Инфляционное обновление.
    pub fn increment(&mut self, actor: A) {
        self.increment_by(actor, 1);
    }

    /// Увеличить вклад актора на `amount`. Инфляционное обновление.
    pub fn increment_by(&mut self, actor: A, amount: u64) {
        *self.counts.entry(actor).or_insert(0) += amount;
    }

    /// Текущее значение счётчика: сумма вкладов всех акторов.
    pub fn value(&self) -> u64 {
        self.counts.values().sum()
    }
}

impl<A: Actor> JoinSemilattice for GCounter<A> {
    /// Покомпонентный максимум вкладов.
    fn join(&mut self, other: &Self) {
        for (actor, &n) in &other.counts {
            let e = self.counts.entry(actor.clone()).or_insert(0);
            *e = (*e).max(n);
        }
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
        // повторная доставка того же состояния не меняет значения
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
