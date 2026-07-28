//! Тесты op-based модели (CmRDT) с мини-моделью причинного broadcast.
//!
//! Транспорт моделируется явно: каждая операция снабжается векторными
//! часами источника, доставка на реплику допустима только когда все
//! причинные предшественники уже доставлены (условие причинного
//! broadcast на `VClock`). Проверяется:
//!
//! * [`OpCounter`] сходится при *любом* порядке доставки — его `effect`
//!   коммутирует безусловно;
//! * [`OpOrSet`] сходится при любом **причинно допустимом** порядке
//!   доставки (property-тест по случайным историям и порядкам);
//! * контр-тест: нарушение причинного порядка (Remove раньше своего Add)
//!   разводит реплики op-based OR-Set навсегда — предпосылка о доставке
//!   не выразима в типах и живёт в транспорте.

use crdt_lab::cmrdt::{CmRdt, OpCounter, OpOrSet};
use crdt_lab::{JoinSemilattice, VClock};
use proptest::collection::vec;
use proptest::prelude::*;

// --- Мини-модель причинного broadcast ---

/// Сообщение: операция с причинной меткой источника.
struct Message<O> {
    origin: usize,
    clock: VClock<usize>,
    op: O,
}

/// Сеть из `n` реплик с журналом всех отправленных операций.
struct CausalNet<C: CmRdt> {
    replicas: Vec<C>,
    /// Причинные часы доставленного на каждой реплике.
    clocks: Vec<VClock<usize>>,
    log: Vec<Message<C::Op>>,
    /// Для каждой реплики — множество индексов доставленных сообщений.
    delivered: Vec<std::collections::BTreeSet<usize>>,
}

impl<C: CmRdt + Default> CausalNet<C> {
    fn new(n: usize) -> Self {
        Self {
            replicas: (0..n).map(|_| C::default()).collect(),
            clocks: vec![VClock::new(); n],
            log: Vec::new(),
            delivered: vec![Default::default(); n],
        }
    }

    /// Локальная операция: источник применяет `effect` немедленно
    /// и рассылает операцию с текущими часами.
    fn local(&mut self, r: usize, op: C::Op) {
        self.clocks[r].increment(r);
        self.replicas[r].effect(&op);
        self.delivered[r].insert(self.log.len());
        self.log.push(Message {
            origin: r,
            clock: self.clocks[r].clone(),
            op,
        });
    }

    /// Условие причинного broadcast: следующее по порядку сообщение
    /// источника, все чужие предшественники уже доставлены.
    fn admissible(&self, r: usize, i: usize) -> bool {
        if self.delivered[r].contains(&i) {
            return false;
        }
        let m = &self.log[i];
        m.clock.get(&m.origin) == self.clocks[r].get(&m.origin) + 1
            && (0..self.replicas.len())
                .filter(|a| *a != m.origin)
                .all(|a| m.clock.get(&a) <= self.clocks[r].get(&a))
    }

    fn admissible_at(&self, r: usize) -> Vec<usize> {
        (0..self.log.len())
            .filter(|&i| self.admissible(r, i))
            .collect()
    }

    fn deliver(&mut self, r: usize, i: usize) {
        assert!(self.admissible(r, i), "доставка вне причинного порядка");
        let op = self.log[i].op.clone();
        let clock = self.log[i].clock.clone();
        self.replicas[r].effect(&op);
        self.clocks[r].join(&clock);
        self.delivered[r].insert(i);
    }

    /// Доставить всё оставшееся в псевдослучайном причинно допустимом
    /// порядке, определяемом `seed`.
    fn flush(&mut self, mut seed: u64) {
        loop {
            let mut progress = false;
            for r in 0..self.replicas.len() {
                let adm = self.admissible_at(r);
                if !adm.is_empty() {
                    seed = seed
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    self.deliver(r, adm[(seed >> 33) as usize % adm.len()]);
                    progress = true;
                }
            }
            if !progress {
                break;
            }
        }
    }

    fn all_converged(&self) -> bool {
        self.replicas.windows(2).all(|w| w[0] == w[1])
    }
}

// --- OpCounter: порядок доставки безразличен ---

proptest! {
    /// `effect` счётчика коммутирует безусловно: любые перестановки
    /// одного набора операций дают одно состояние (причинность не нужна).
    #[test]
    fn opcounter_converges_under_any_delivery_order(
        (deltas, shuffled) in vec(-5i64..=5, 1..16)
            .prop_flat_map(|v| (Just(v.clone()), Just(v).prop_shuffle()))
    ) {
        let mut a = OpCounter::new();
        let mut b = OpCounter::new();
        for d in &deltas {
            a.effect(d);
        }
        for d in &shuffled {
            b.effect(d);
        }
        prop_assert_eq!(a, b);
    }
}

// --- OpOrSet: сходимость при причинной доставке ---

/// Шаг случайной истории: локальная операция или доставка.
#[derive(Debug, Clone)]
enum Cmd {
    Add(u8, u8),
    Remove(u8, u8),
    /// Доставить на реплику один из допустимых пакетов (выбор по модулю).
    Deliver(u8, u8),
}

fn cmds() -> impl Strategy<Value = Vec<Cmd>> {
    vec(
        prop_oneof![
            ((0u8..3), (0u8..4)).prop_map(|(r, x)| Cmd::Add(r, x)),
            ((0u8..3), (0u8..4)).prop_map(|(r, x)| Cmd::Remove(r, x)),
            ((0u8..3), any::<u8>()).prop_map(|(r, k)| Cmd::Deliver(r, k)),
        ],
        0..40,
    )
}

proptest! {
    /// Любая история операций и любой причинно допустимый порядок
    /// доставки приводят все реплики в одно состояние.
    #[test]
    fn op_orset_converges_under_causal_delivery(cmds in cmds(), seed in any::<u64>()) {
        let mut net: CausalNet<OpOrSet<usize, u8>> = CausalNet::new(3);
        for cmd in cmds {
            match cmd {
                Cmd::Add(r, x) => {
                    let r = r as usize;
                    let op = net.replicas[r].prepare_add(r, x);
                    net.local(r, op);
                }
                Cmd::Remove(r, x) => {
                    let r = r as usize;
                    let op = net.replicas[r].prepare_remove(&x);
                    net.local(r, op);
                }
                Cmd::Deliver(r, k) => {
                    let r = r as usize;
                    let adm = net.admissible_at(r);
                    if !adm.is_empty() {
                        net.deliver(r, adm[k as usize % adm.len()]);
                    }
                }
            }
        }
        net.flush(seed);
        prop_assert!(net.all_converged());
    }
}

/// Add-wins сохраняется и в op-based варианте: конкурентная вставка
/// несёт метку, которой удаление не наблюдало.
#[test]
fn op_orset_add_wins_under_causal_delivery() {
    let mut net: CausalNet<OpOrSet<usize, &str>> = CausalNet::new(2);
    let add = net.replicas[0].prepare_add(0, "x");
    net.local(0, add);
    net.deliver(1, 0); // реплика 1 наблюдает вставку…
    let rem = net.replicas[1].prepare_remove(&"x");
    net.local(1, rem); // …и удаляет её
    let readd = net.replicas[0].prepare_add(0, "x"); // конкурентная вставка
    net.local(0, readd);
    net.flush(42);
    assert!(net.all_converged());
    assert!(net.replicas[0].contains(&"x")); // add-wins
}

// --- Контр-тест: без причинной доставки op-based OR-Set расходится ---

/// Одна и та же пара операций, доставленная в разном порядке, разводит
/// реплики навсегда: `Remove` раньше «своего» `Add` удаляет пустоту,
/// а затем `Add` воскрешает элемент. У state-based OR-Set такой сценарий
/// невозможен: надгробие — часть растущего состояния, и `⊔` идемпотентен
/// и коммутативен для любых порядков слияния.
#[test]
fn op_orset_without_causal_delivery_diverges() {
    let mut source: OpOrSet<u8, &str> = OpOrSet::new();
    let add = source.prepare_add(0, "x");
    source.effect(&add);
    let rem = source.prepare_remove(&"x"); // причинно следует за add
    source.effect(&rem);

    // Причинный порядок: add, затем rem — совпадает с источником.
    let mut causal: OpOrSet<u8, &str> = OpOrSet::new();
    causal.effect(&add);
    causal.effect(&rem);
    assert_eq!(causal, source);
    assert!(!causal.contains(&"x"));

    // Нарушенный порядок: rem доставлен раньше add.
    let mut reordered: OpOrSet<u8, &str> = OpOrSet::new();
    reordered.effect(&rem); // удалять нечего
    reordered.effect(&add); // элемент «воскресает»
    assert!(reordered.contains(&"x"));
    assert_ne!(causal, reordered); // один набор операций — разные состояния
}
