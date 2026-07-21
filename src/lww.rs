//! LWW-Register — регистр «последняя запись побеждает».

use crate::lattice::{Actor, JoinSemilattice, Update};

/// Регистр, разрешающий конфликты по принципу *last-writer-wins*.
///
/// Состояние — тройка `(timestamp, actor, value)`; `join` выбирает
/// лексикографически бóльшую тройку. Лексикографический порядок на
/// произведении **линейных** порядков линеен, а `max` в линейном порядке —
/// это `join` полурешётки, так что законы выполняются автоматически.
///
/// Разрешение конфликтов детерминировано на уровне типов:
///
/// * при равных метках времени побеждает больший идентификатор актора
///   (`A: Ord` требует система типов — регистр с несравнимыми акторами
///   просто не скомпилируется);
/// * запасной третий компонент `T: Ord` делает `join` тотальным даже при
///   нарушении протокольного инварианта «пара `(timestamp, actor)`
///   уникальна» — законы полурешётки верны для *любых* состояний, что
///   упрощает и усиливает property-based тесты.
///
/// Семантическая цена LWW — потерянные обновления: из конкурентных записей
/// выживает одна. Это свойство типа, а не дефект реализации.
///
/// # Пример
///
/// ```
/// use crdt_lab::{LwwRegister, CvRdt};
///
/// let mut a = LwwRegister::new("a", "черновик");
/// let mut b = a.clone();
/// a.write(1, "a", "версия A");
/// b.write(2, "b", "версия B");
///
/// a.merge(&b);
/// b.merge(&a);
/// assert_eq!(a, b);
/// assert_eq!(*a.value(), "версия B"); // большая метка времени победила
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LwwRegister<A: Actor, T: Ord + Clone> {
    timestamp: u64,
    actor: A,
    value: T,
}

impl<A: Actor, T: Ord + Clone> LwwRegister<A, T> {
    /// Регистр с начальным значением и меткой времени 0.
    pub fn new(actor: A, value: T) -> Self {
        Self {
            timestamp: 0,
            actor,
            value,
        }
    }

    /// Текущее значение.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Метка времени текущего значения.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Записать значение с меткой времени.
    ///
    /// Реализована как `join` с одноэлементным состоянием, поэтому
    /// инфляционность выполняется по построению: «устаревшая» запись
    /// (с меньшей тройкой) просто не изменит регистр.
    pub fn write(&mut self, timestamp: u64, actor: A, value: T) {
        self.join(&Self {
            timestamp,
            actor,
            value,
        });
    }

    /// Ключ сравнения состояний.
    fn key(&self) -> (u64, &A, &T) {
        (self.timestamp, &self.actor, &self.value)
    }
}

/// Начальное состояние из значений по умолчанию (метка времени 0).
///
/// Нужен обобщённому коду (например, свёртке операций в тестах);
/// наименьшим элементом решётки в общем случае **не** является,
/// поскольку `A::default()`/`T::default()` не обязаны быть минимумами.
impl<A: Actor + Default, T: Ord + Clone + Default> Default for LwwRegister<A, T> {
    fn default() -> Self {
        Self::new(A::default(), T::default())
    }
}

impl<A: Actor, T: Ord + Clone> Update for LwwRegister<A, T> {
    /// Операция — тройка «метка времени, актор, значение».
    type Op = (u64, A, T);

    fn apply(&mut self, (timestamp, actor, value): Self::Op) {
        self.write(timestamp, actor, value);
    }
}

impl<A: Actor, T: Ord + Clone> JoinSemilattice for LwwRegister<A, T> {
    /// Максимум по лексикографическому порядку `(timestamp, actor, value)`.
    fn join(&mut self, other: &Self) {
        if self.key() < other.key() {
            self.clone_from(other);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_timestamp_wins() {
        let mut r = LwwRegister::new("a", 0);
        r.write(5, "a", 42);
        r.write(3, "b", 7); // устаревшая запись игнорируется
        assert_eq!(*r.value(), 42);
    }

    #[test]
    fn actor_breaks_timestamp_tie() {
        let mut a = LwwRegister::new("a", 0);
        let mut b = a.clone();
        a.write(1, "a", 10);
        b.write(1, "b", 20);
        let m = a.clone().joined(&b);
        assert_eq!(m, b.joined(&a));
        assert_eq!(*m.value(), 20); // "b" > "a"
    }
}
