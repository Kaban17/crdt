//! Op-based модель — CmRDT (commutative replicated data type).
//!
//! Второй класс CRDT по Shapiro et al.: реплики обмениваются не
//! состояниями, а **операциями**. Обновление двухфазно:
//!
//! 1. *prepare* — выполняется только на источнике, читает локальное
//!    состояние и изготавливает операцию (например, снабжает вставку
//!    свежей меткой);
//! 2. *effect* — применяется на **каждой** реплике, включая источник.
//!
//! Сходимость требует, чтобы `effect` конкурентных операций коммутировал,
//! **и** чтобы транспорт доставлял операции в причинном порядке и ровно
//! один раз. Контраст с CvRDT — центральный для сравнительной главы:
//!
//! * законы полурешётки (ACI) — свойства *типа*, проверяемые обобщённо
//!   для любого `T: JoinSemilattice`; идемпотентность `⊔` прощает
//!   транспорту дубли и пересечения состояний;
//! * предпосылки op-based модели — свойства *транспорта*: причинную
//!   доставку и exactly-once нельзя навесить на тип операции трейтом,
//!   в сигнатуре [`CmRdt::effect`] их просто негде записать. Это честная
//!   граница подхода «спецификация в типах» (мост к Propel/VeriFx,
//!   где предпосылки доставки — часть верифицируемой спецификации).
//!
//! `prepare`-фаза остаётся методами конкретных типов
//! ([`OpOrSet::prepare_add`] и т.п.): её сигнатура типоспецифична,
//! общего трейта для неё нет.
//!
//! Модель причинного broadcast и тесты сходимости/расходимости —
//! `tests/cmrdt.rs`.

use std::collections::{BTreeMap, BTreeSet};

use crate::lattice::Actor;
use crate::orset::Dot;

/// Операционный (op-based) CRDT: фаза *effect*.
///
/// # Контракт (проверяется тестами, не компилятором)
///
/// Для операций `p`, `q`, изготовленных **конкурентно** (ни одна не
/// наблюдала другую): `effect(effect(s, p), q) = effect(effect(s, q), p)`.
///
/// Для причинно упорядоченных операций коммутативность не требуется —
/// но тогда корректность зависит от предположения, что транспорт
/// доставляет их в причинном порядке (см. контр-тест расходимости в
/// `tests/cmrdt.rs`). В отличие от [`JoinSemilattice::join`]
/// (идемпотентного), `effect` в общем случае не идемпотентен:
/// повторная доставка — тоже обязанность транспорта.
///
/// [`JoinSemilattice::join`]: crate::JoinSemilattice::join
pub trait CmRdt: Clone + Eq {
    /// Тип передаваемой операции.
    type Op: Clone;

    /// Применить доставленную операцию к состоянию.
    fn effect(&mut self, op: &Self::Op);
}

/// Op-based счётчик: простейший CmRDT.
///
/// Операция — целочисленная дельта; сложение коммутативно и без
/// предположений о конкурентности, поэтому счётчик сходится при *любом*
/// порядке доставки — причинный broadcast ему не нужен (но exactly-once
/// нужен по-прежнему: `+δ` не идемпотентно). Сравните с [`OpOrSet`],
/// где коммутативность держится только на причинной доставке.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct OpCounter {
    value: i64,
}

impl OpCounter {
    /// Нулевой счётчик.
    pub fn new() -> Self {
        Self { value: 0 }
    }

    /// Текущее значение.
    pub fn value(&self) -> i64 {
        self.value
    }

    /// Фаза *prepare*: изготовить операцию-дельту.
    ///
    /// Состояние не читается — вырожденный случай, объясняющий, почему
    /// счётчику безразличен порядок доставки.
    pub fn prepare(&self, delta: i64) -> i64 {
        delta
    }
}

impl CmRdt for OpCounter {
    type Op = i64;

    fn effect(&mut self, op: &Self::Op) {
        self.value += op;
    }
}

/// Операция над [`OpOrSet`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Op<A: Actor, T> {
    /// Вставка: элемент со свежей меткой, выданной `prepare`-фазой источника.
    Add {
        /// Свежая уникальная метка вставки.
        dot: Dot<A>,
        /// Вставляемый элемент.
        item: T,
    },
    /// Удаление: множество меток, *наблюдавшихся* у элемента на источнике.
    Remove {
        /// Наблюдённые на источнике метки вставок элемента.
        dots: BTreeSet<Dot<A>>,
        /// Удаляемый элемент.
        item: T,
    },
}

/// Op-based OR-Set (add-wins).
///
/// В отличие от state-based [`OrSet`](crate::OrSet), надгробия не нужны:
/// `Remove` несёт явный список наблюдённых меток, и `effect` физически
/// удаляет их. Add-wins получается так же, как в state-based варианте:
/// конкурентная вставка несёт метку, которой удаление не наблюдало.
///
/// Цена — коммутативность `effect` держится на **причинной доставке**:
/// если `Remove(d)` доставлен раньше, чем породивший `d` `Add`, удаление
/// пройдёт вхолостую, а затем `Add` «воскресит» элемент — реплики
/// разойдутся навсегда (контр-тест в `tests/cmrdt.rs`). У state-based
/// OR-Set эта ситуация невозможна по построению: надгробие — часть
/// растущего состояния, `⊔` переживает любой порядок слияний.
///
/// Модель предполагает, что источник немедленно применяет `effect`
/// изготовленной операции к себе (доставка себе мгновенна); `prepare_add`
/// без промежуточного `effect` выдаст повторную метку.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct OpOrSet<A: Actor, T: Ord + Clone> {
    /// Живые метки вставок каждого элемента (пустых записей нет).
    entries: BTreeMap<T, BTreeSet<Dot<A>>>,
    /// Максимальный виденный номер метки каждого актора — база
    /// `prepare`-фазы для выдачи свежих меток.
    counters: BTreeMap<A, u64>,
}

impl<A: Actor, T: Ord + Clone> OpOrSet<A, T> {
    /// Пустое множество.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            counters: BTreeMap::new(),
        }
    }

    /// Фаза *prepare* вставки: снабдить элемент свежей меткой актора.
    pub fn prepare_add(&self, actor: A, item: T) -> Op<A, T> {
        let next = self.counters.get(&actor).copied().unwrap_or(0) + 1;
        Op::Add {
            dot: Dot::new(actor, next),
            item,
        }
    }

    /// Фаза *prepare* удаления: собрать наблюдаемые метки элемента.
    pub fn prepare_remove(&self, item: &T) -> Op<A, T> {
        Op::Remove {
            dots: self.entries.get(item).cloned().unwrap_or_default(),
            item: item.clone(),
        }
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
}

impl<A: Actor, T: Ord + Clone> CmRdt for OpOrSet<A, T> {
    type Op = Op<A, T>;

    fn effect(&mut self, op: &Self::Op) {
        match op {
            Op::Add { dot, item } => {
                self.entries
                    .entry(item.clone())
                    .or_default()
                    .insert(dot.clone());
                let seen = self.counters.entry(dot.actor().clone()).or_insert(0);
                *seen = (*seen).max(dot.counter());
            }
            Op::Remove { dots, item } => {
                if let Some(live) = self.entries.get_mut(item) {
                    for dot in dots {
                        live.remove(dot);
                    }
                    if live.is_empty() {
                        self.entries.remove(item);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_sums_deltas() {
        let mut c = OpCounter::new();
        c.effect(&c.prepare(3));
        c.effect(&c.prepare(-1));
        assert_eq!(c.value(), 2);
    }

    #[test]
    fn add_then_remove_locally() {
        let mut s: OpOrSet<&str, i32> = OpOrSet::new();
        let add = s.prepare_add("a", 1);
        s.effect(&add);
        assert!(s.contains(&1));
        let rem = s.prepare_remove(&1);
        s.effect(&rem);
        assert!(!s.contains(&1));
        assert!(s.is_empty());
    }

    #[test]
    fn prepare_add_issues_fresh_dots() {
        let mut s: OpOrSet<&str, i32> = OpOrSet::new();
        let first = s.prepare_add("a", 1);
        s.effect(&first);
        let second = s.prepare_add("a", 1);
        assert_ne!(first, second); // метки различны, состояние их различает
    }
}
