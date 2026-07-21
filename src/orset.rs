//! OR-Set — множество с наблюдаемым удалением (observed-remove, add-wins).

use std::collections::{BTreeMap, BTreeSet};

use crate::lattice::{Actor, JoinSemilattice, Update};

/// Операция над [`OrSet`].
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
/// Плата — неограниченный рост надгробий; оптимизированные варианты
/// (с причинным контекстом вместо надгробий) — направление для
/// дальнейшей работы.
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
