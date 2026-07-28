//! MV-Register — многозначный регистр (multi-value register).

use std::fmt;

use crate::lattice::{Actor, JoinSemilattice, Update, ValidateMerge};
use crate::vclock::VClock;

/// Запись регистра: значение с причинным контекстом.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Entry<A: Actor, T> {
    clock: VClock<A>,
    value: T,
}

/// Многозначный регистр: конкурентные записи **не теряются**.
///
/// Антипод [`LwwRegister`](crate::LwwRegister): вместо детерминированного
/// выбора одного «победителя» регистр хранит *все* причинно-несравнимые
/// значения и отдаёт их читателю на разрешение (как корзины Dynamo).
///
/// Состояние — **антицепь** пар `(векторные часы, значение)`: ни одни
/// часы не доминируют другие строго. Запись наблюдает все текущие часы
/// (берёт их `⊔` и увеличивает свою компоненту), поэтому вытесняет всё
/// увиденное; слияние оставляет максимальные элементы объединения —
/// конкурентные записи выживают обе.
///
/// Для канонического представления (нужно `Eq` законов решётки) записи
/// хранятся отсортированными по произвольному тотальному порядку часов
/// и дедуплицированными.
///
/// # Пример
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
/// assert_eq!(seen, [1, 2]); // конкурентные значения сохранены оба
///
/// a.write("a", 3); // эта запись наблюдала оба значения…
/// assert_eq!(a.read().copied().collect::<Vec<_>>(), [3]); // …и вытеснила их
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
    /// Пустой регистр — наименьший элемент полурешётки.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Текущие значения: все причинно-несравнимые записи.
    pub fn read(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().map(|e| &e.value)
    }

    /// Записи вместе с причинным контекстом (для наблюдения за состоянием).
    pub fn read_with_clocks(&self) -> impl Iterator<Item = (&VClock<A>, &T)> {
        self.entries.iter().map(|e| (&e.clock, &e.value))
    }

    /// Число хранимых конкурентных значений.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Пуст ли регистр (записей ещё не было).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Записать значение от имени актора.
    ///
    /// Часы новой записи — `⊔` всех наблюдаемых часов плюс собственное
    /// событие, поэтому запись строго доминирует всё текущее состояние:
    /// операция инфляционна, а всё наблюдаемое вытесняется.
    pub fn write(&mut self, actor: A, value: T) {
        let mut clock = VClock::new();
        for e in &self.entries {
            clock.join(&e.clock);
        }
        clock.increment(actor);
        self.entries = vec![Entry { clock, value }];
    }

    /// Канонизация: убрать строго доминированные записи (оставить
    /// антицепь), отсортировать и дедуплицировать.
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
    /// Максимальные элементы объединения двух антицепей.
    fn join(&mut self, other: &Self) {
        let mut candidates = std::mem::take(&mut self.entries);
        candidates.extend(other.entries.iter().cloned());
        self.entries = Self::canonicalize(candidates);
    }
}

/// Нарушение инварианта MV-регистра: одни и те же векторные часы
/// сопровождают разные значения.
///
/// Часы записи однозначно определяют событие (актор + номер его события),
/// поэтому отображение «часы → значение» обязано быть инъективным.
/// Коллизия означает, что актор выдал один номер события двум записям —
/// после слияния обе останутся в антицепи как «конкурентные», хотя
/// причинно совпадают.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MergeError<A: Actor, T: Ord + Clone> {
    /// Часы, встретившиеся с разными значениями.
    pub clock: VClock<A>,
    /// Противоречащие значения из `self` и `other`.
    pub values: (T, T),
}

impl<A: Actor + fmt::Debug, T: Ord + Clone + fmt::Debug> fmt::Display for MergeError<A, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "часы {:?} сопровождают разные значения: {:?} и {:?}",
            self.clock, self.values.0, self.values.1
        )
    }
}

impl<A: Actor + fmt::Debug, T: Ord + Clone + fmt::Debug> std::error::Error for MergeError<A, T> {}

impl<A: Actor, T: Ord + Clone> ValidateMerge for MvRegister<A, T> {
    type Error = MergeError<A, T>;

    /// Ошибка, если в двух состояниях есть записи с равными часами,
    /// но разными значениями.
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
    /// Операция — пара «актор, значение».
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
        // и это состояние доминирует прежние
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
        // повторная доставка старого состояния ничего не воскрешает
        a.join(&b);
        assert_eq!(sorted_read(&a), [3]);
    }
}
