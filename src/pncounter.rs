//! PN-Counter — счётчик с инкрементом и декрементом.

use crate::gcounter::GCounter;
use crate::lattice::{Actor, JoinSemilattice};

/// Счётчик с увеличением *и* уменьшением.
///
/// Декремент — не инфляционная операция для обычного числа, поэтому тип
/// строится как **произведение двух полурешёток**: пара растущих счётчиков
/// «плюсов» и «минусов». Произведение полурешёток с покомпонентным `join`
/// снова полурешётка — это свойство композиции библиотека использует
/// напрямую: `join` пары делегирует в `join` компонент, и законы
/// наследуются по построению.
///
/// Значение — разность `plus - minus` (вычисляется в `i64`).
///
/// # Пример
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
    /// Нулевой счётчик.
    pub fn new() -> Self {
        Self {
            plus: GCounter::new(),
            minus: GCounter::new(),
        }
    }

    /// Увеличить счётчик на 1 от имени актора.
    pub fn increment(&mut self, actor: A) {
        self.plus.increment(actor);
    }

    /// Уменьшить счётчик на 1 от имени актора.
    ///
    /// В решётке состояний операция всё же *инфляционная*: растёт
    /// компонента `minus`, хотя видимое значение убывает. Разделение
    /// «порядок состояний» / «видимое значение» — ключ к декременту.
    pub fn decrement(&mut self, actor: A) {
        self.minus.increment(actor);
    }

    /// Текущее значение: сумма плюсов минус сумма минусов.
    pub fn value(&self) -> i64 {
        self.plus.value() as i64 - self.minus.value() as i64
    }
}

impl<A: Actor> JoinSemilattice for PnCounter<A> {
    /// Покомпонентный `join` произведения полурешёток.
    fn join(&mut self, other: &Self) {
        self.plus.join(&other.plus);
        self.minus.join(&other.minus);
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
        assert!(before.leq(&c)); // состояние выросло
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
