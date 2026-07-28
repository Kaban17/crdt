//! ORSWOT — OR-Set без надгробий (observed-remove set without tombstones).

use std::collections::{BTreeMap, BTreeSet};

use crate::lattice::{Actor, JoinSemilattice, Update};
use crate::orset::{Dot, Op};
use crate::vclock::VClock;

/// Оптимизированный OR-Set: причинный контекст вместо надгробий
/// (Bieniusa et al., 2012; реализация Riak).
///
/// У state-based [`OrSet`](crate::OrSet) удаление добавляет метку в вечно
/// растущее множество надгробий. ORSWOT заменяет их **причинным
/// контекстом** — векторными часами всех когда-либо *виденных* меток:
///
/// * `add` выдаёт свежую метку, продвигая контекст;
/// * `remove` **физически** удаляет метки элемента — контекст помнит,
///   что они были виденны;
/// * отсутствие метки в состоянии имеет два смысла, и различает их
///   контекст: «ещё не видел» (метка вне контекста) либо «видел и
///   удалил» (метка покрыта контекстом).
///
/// Поэтому `join` — **не** покомпонентное объединение: метка выживает,
/// если она есть в обоих состояниях, либо есть в одном и *не покрыта*
/// контекстом другого (тот её ещё не видел — конкурентная вставка,
/// add-wins). Метка, присутствующая в одном состоянии, но покрытая
/// контекстом другого, была там удалена — и умирает при слиянии.
///
/// Инструктивно, что `remove` *уменьшает* состояние структурно, но
/// остаётся **инфляционной** операцией выводимого порядка
/// (`a ≤ b ⟺ a ⊔ b = b`): порядок полурешётки измеряет не размер,
/// а *знание* — «видел и удалил» строго больше, чем «видел». Раз так,
/// повторная доставка старого состояния ничего не воскрешает.
///
/// Размер состояния ограничен: живые метки плюс по одному счётчику на
/// актора — след истории add/remove-циклов не накапливается
/// (см. тест `state_stays_bounded_after_add_remove_cycles`).
///
/// # Пример
///
/// ```
/// use crdt_lab::{Orswot, CvRdt};
///
/// let mut a: Orswot<&str, &str> = Orswot::new();
/// a.add("a", "x");
/// let mut b = a.clone();
///
/// b.remove(&"x");   // b удаляет наблюдаемый "x" физически
/// a.add("a", "x");  // a конкурентно вставляет "x" заново
///
/// a.merge(&b);
/// assert!(a.contains(&"x")); // add-wins: свежая метка не покрыта контекстом b
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Orswot<A: Actor, T: Ord + Clone> {
    /// Живые метки вставок каждого элемента (пустых записей нет).
    entries: BTreeMap<T, BTreeSet<Dot<A>>>,
    /// Причинный контекст: все когда-либо виденные метки каждого актора.
    context: VClock<A>,
}

impl<A: Actor, T: Ord + Clone> Orswot<A, T> {
    /// Пустое множество.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            context: VClock::new(),
        }
    }

    /// Вставить элемент от имени актора, пометив свежей меткой.
    pub fn add(&mut self, actor: A, item: T) {
        let next = self.context.get(&actor) + 1;
        self.context.increment(actor.clone());
        self.entries
            .entry(item)
            .or_default()
            .insert(Dot::new(actor, next));
    }

    /// Удалить элемент: физически стереть его метки.
    ///
    /// Контекст не меняется — он и так покрывает стираемые метки, и
    /// именно это отличит «удалено» от «не видел» при слиянии.
    pub fn remove(&mut self, item: &T) {
        self.entries.remove(item);
    }

    /// Присутствует ли элемент.
    pub fn contains(&self, item: &T) -> bool {
        self.entries.contains_key(item)
    }

    /// Итератор по присутствующим элементам в возрастающем порядке.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.entries.keys()
    }

    /// Число присутствующих элементов.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Пусто ли множество.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Причинный контекст (для наблюдения за размером состояния).
    pub fn context(&self) -> &VClock<A> {
        &self.context
    }

    /// Число живых меток — вместе с контекстом это весь размер состояния
    /// (сравните с [`OrSet::tombstone_count`](crate::OrSet::tombstone_count)).
    pub fn dot_count(&self) -> usize {
        self.entries.values().map(BTreeSet::len).sum()
    }

    /// Выживает ли метка `dot` элемента `item` из состояния `src`
    /// при слиянии с состоянием `dst`.
    fn survives(dot: &Dot<A>, item: &T, dst: &Self) -> bool {
        dst.entries.get(item).is_some_and(|dots| dots.contains(dot))
            || dot.counter() > dst.context.get(dot.actor())
    }
}

impl<A: Actor, T: Ord + Clone> JoinSemilattice for Orswot<A, T> {
    /// Слияние по правилу «жива в обоих, либо жива в одном и не видена
    /// другим»; контексты объединяются покомпонентным максимумом.
    fn join(&mut self, other: &Self) {
        let mut merged: BTreeMap<T, BTreeSet<Dot<A>>> = BTreeMap::new();
        for (item, dots) in &self.entries {
            let kept: BTreeSet<Dot<A>> = dots
                .iter()
                .filter(|dot| Self::survives(dot, item, other))
                .cloned()
                .collect();
            if !kept.is_empty() {
                merged.insert(item.clone(), kept);
            }
        }
        for (item, dots) in &other.entries {
            for dot in dots.iter().filter(|dot| Self::survives(dot, item, self)) {
                merged.entry(item.clone()).or_default().insert(dot.clone());
            }
        }
        self.entries = merged;
        self.context.join(&other.context);
    }
}

impl<A: Actor, T: Ord + Clone> Update for Orswot<A, T> {
    /// Тот же алфавит операций, что у [`OrSet`](crate::OrSet), — общие
    /// стратегии тестов и прямое сравнение двух реализаций.
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
    use crate::OrSet;

    #[test]
    fn add_then_remove_locally() {
        let mut s: Orswot<&str, i32> = Orswot::new();
        s.add("a", 1);
        assert!(s.contains(&1));
        s.remove(&1);
        assert!(!s.contains(&1));
        assert!(s.is_empty());
    }

    #[test]
    fn concurrent_add_wins_over_remove() {
        let mut a: Orswot<&str, i32> = Orswot::new();
        a.add("a", 1);
        let mut b = a.clone();

        b.remove(&1);
        a.add("a", 1); // конкурентная повторная вставка

        let m = a.joined(&b);
        assert!(m.contains(&1));
    }

    #[test]
    fn remove_of_unseen_element_loses_to_add() {
        let mut a: Orswot<&str, i32> = Orswot::new();
        let mut b: Orswot<&str, i32> = Orswot::new();
        b.add("b", 1);
        a.remove(&1); // a не видела вставку — стирать нечего
        let m = a.joined(&b);
        assert!(m.contains(&1));
    }

    #[test]
    fn observed_remove_kills_dot_on_merge() {
        let mut a: Orswot<&str, i32> = Orswot::new();
        a.add("a", 1);
        let stale = a.clone(); // старое состояние с живой меткой
        a.remove(&1);
        // повторная доставка старого состояния не воскрешает элемент:
        // его метка покрыта контекстом a
        let m = a.joined(&stale);
        assert!(!m.contains(&1));
    }

    /// Структурно remove уменьшает состояние, но в выводимом порядке
    /// полурешётки оно растёт: порядок измеряет знание, а не размер.
    #[test]
    fn remove_is_inflationary_in_derived_order() {
        let mut s: Orswot<&str, i32> = Orswot::new();
        s.add("a", 1);
        let before = s.clone();
        s.remove(&1);
        assert!(before.leq(&s));
        assert!(!s.leq(&before));
    }

    /// Центральное отличие от надгробного OR-Set: след истории не
    /// накапливается, размер состояния ограничен живыми метками плюс
    /// одним счётчиком на актора.
    #[test]
    fn state_stays_bounded_after_add_remove_cycles() {
        let mut swot: Orswot<&str, i32> = Orswot::new();
        let mut orset: OrSet<&str, i32> = OrSet::new();
        for _ in 0..100 {
            swot.add("a", 1);
            swot.remove(&1);
            orset.add("a", 1);
            orset.remove(&1);
        }
        // надгробный OR-Set накопил след каждой вставки…
        assert_eq!(orset.tombstone_count(), 100);
        // …ORSWOT — ноль живых меток и один счётчик контекста
        assert_eq!(swot.dot_count(), 0);
        assert_eq!(swot.context().get(&"a"), 100);
        assert!(swot.is_empty());
    }
}
