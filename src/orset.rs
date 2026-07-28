//! OR-Set — множество с наблюдаемым удалением (observed-remove, add-wins).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::lattice::{Actor, JoinSemilattice, Update, ValidateMerge};

/// Операция над [`OrSet`] (и [`Orswot`](crate::Orswot) — общий алфавит
/// операций позволяет сравнивать реализации на одних историях).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Op<A: Actor, T> {
    /// Вставить элемент от имени актора.
    Add(A, T),
    /// Удалить все наблюдаемые вхождения элемента.
    Remove(T),
}

/// Уникальная метка события вставки: пара `(актор, порядковый номер)`.
///
/// Newtype-обёртка не даёт перепутать метку с «обычной» парой и
/// централизует инвариант уникальности: свежие метки выдаёт только
/// [`OrSet::add`], увеличивая личный счётчик актора.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Dot<A: Actor> {
    actor: A,
    counter: u64,
}

impl<A: Actor> Dot<A> {
    /// Конструктор для op-based варианта ([`crate::cmrdt::OpOrSet`]):
    /// там свежесть метки обеспечивает `prepare`-фаза источника, а не
    /// этот модуль. Намеренно `pub(crate)`: снаружи крейта метки
    /// по-прежнему невозможно изготовить вручную.
    pub(crate) fn new(actor: A, counter: u64) -> Self {
        Self { actor, counter }
    }

    /// Актор, выдавший метку.
    pub fn actor(&self) -> &A {
        &self.actor
    }

    /// Порядковый номер метки у актора.
    pub fn counter(&self) -> u64 {
        self.counter
    }
}

/// Множество с поддержкой удаления и семантикой **add-wins**:
/// при конкурентных `add(x)` и `remove(x)` элемент остаётся.
///
/// Наивное «множество с удалением» полурешёткой не является: удаление —
/// не инфляционная операция. OR-Set решает это переводом обеих операций
/// в рост состояния:
///
/// * `add(x)` помечает вставку свежей уникальной меткой ([`Dot`]);
/// * `remove(x)` не трогает `x`, а добавляет в множество-надгробие
///   (*tombstones*) **наблюдаемые** метки вставок `x`;
/// * элемент присутствует, если у него есть метка вне надгробий.
///
/// Все три компоненты состояния (метки вставок, надгробия, счётчики
/// акторов) — растущие структуры, их произведение — снова полурешётка.
/// Add-wins следует из определения: конкурентная вставка несёт метку,
/// которую удаление не наблюдало и потому не похоронило.
///
/// Плата — неограниченный рост надгробий; оптимизированный вариант с
/// причинным контекстом вместо надгробий — [`Orswot`](crate::Orswot).
///
/// # Пример
///
/// ```
/// use crdt_lab::{OrSet, CvRdt};
///
/// let mut a: OrSet<&str, &str> = OrSet::new();
/// a.add("a", "x");
/// let mut b = a.clone();
///
/// b.remove(&"x");   // b удаляет наблюдаемый "x"
/// a.add("a", "x");  // a конкурентно вставляет "x" заново
///
/// a.merge(&b);
/// assert!(a.contains(&"x")); // add-wins
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct OrSet<A: Actor, T: Ord + Clone> {
    /// Живые метки вставок каждого элемента.
    entries: BTreeMap<T, BTreeSet<Dot<A>>>,
    /// Надгробия: метки, чьё удаление было наблюдено.
    tombstones: BTreeSet<Dot<A>>,
    /// Личные счётчики акторов для выдачи свежих меток.
    counters: BTreeMap<A, u64>,
}

impl<A: Actor, T: Ord + Clone> OrSet<A, T> {
    /// Пустое множество.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            tombstones: BTreeSet::new(),
            counters: BTreeMap::new(),
        }
    }

    /// Вставить элемент от имени актора, пометив свежей меткой.
    pub fn add(&mut self, actor: A, item: T) {
        let counter = self.counters.entry(actor.clone()).or_insert(0);
        *counter += 1;
        let dot = Dot {
            actor,
            counter: *counter,
        };
        self.entries.entry(item).or_default().insert(dot);
    }

    /// Удалить элемент: похоронить все *наблюдаемые* метки его вставок.
    ///
    /// Конкурентные (ещё не доставленные) вставки не затрагиваются —
    /// отсюда семантика add-wins.
    pub fn remove(&mut self, item: &T) {
        if let Some(dots) = self.entries.get(item) {
            self.tombstones.extend(dots.iter().cloned());
        }
    }

    /// Присутствует ли элемент: есть ли у него метка вне надгробий.
    pub fn contains(&self, item: &T) -> bool {
        self.entries
            .get(item)
            .is_some_and(|dots| !dots.is_subset(&self.tombstones))
    }

    /// Итератор по присутствующим элементам в возрастающем порядке.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.entries
            .iter()
            .filter(|(_, dots)| !dots.is_subset(&self.tombstones))
            .map(|(item, _)| item)
    }

    /// Число присутствующих элементов.
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    /// Пусто ли множество (с точки зрения наблюдателя).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Число надгробий — накопленных меток удалённых вставок.
    /// Наблюдаемая метрика неограниченного роста состояния (см. план:
    /// оптимизация через причинный контекст).
    pub fn tombstone_count(&self) -> usize {
        self.tombstones.len()
    }
}

impl<A: Actor, T: Ord + Clone> JoinSemilattice for OrSet<A, T> {
    /// Покомпонентный `join`: объединение меток и надгробий,
    /// покомпонентный максимум счётчиков.
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

/// Нарушение инварианта OR-Set: одна метка [`Dot`] прикреплена к разным
/// элементам в двух состояниях.
///
/// Уникальность меток обеспечивается генератором ([`OrSet::add`]) только
/// в пределах реплики; коллизия означает, что один актор работал на двух
/// репликах одновременно (или счётчик актора был сброшен). После слияния
/// такая метка «склеит» судьбу двух независимых вставок: одно `remove`
/// похоронит обе.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MergeError<A: Actor, T: Ord + Clone> {
    /// Метка, встретившаяся у разных элементов.
    pub dot: Dot<A>,
    /// Элементы, к которым метка прикреплена в `self` и `other`.
    pub items: (T, T),
}

impl<A: Actor + fmt::Debug, T: Ord + Clone + fmt::Debug> fmt::Display for MergeError<A, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "метка {:?} прикреплена к разным элементам: {:?} и {:?}",
            self.dot, self.items.0, self.items.1
        )
    }
}

impl<A: Actor + fmt::Debug, T: Ord + Clone + fmt::Debug> std::error::Error for MergeError<A, T> {}

impl<A: Actor, T: Ord + Clone> ValidateMerge for OrSet<A, T> {
    type Error = MergeError<A, T>;

    /// Ошибка, если какая-то метка вставки прикреплена в двух состояниях
    /// к разным элементам.
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
        a.add("a", 1); // конкурентная повторная вставка

        let m = a.joined(&b);
        assert!(m.contains(&1));
    }

    #[test]
    fn remove_only_observed_dots() {
        let mut a: OrSet<&str, i32> = OrSet::new();
        let mut b: OrSet<&str, i32> = OrSet::new();
        b.add("b", 1);
        // a не видела вставку и её remove ничего не хоронит
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
        a.add("a", 1); // свежая метка не похоронена
        assert!(a.contains(&1));
    }
}
