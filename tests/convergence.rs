//! Имитационная проверка сильной согласованности в конечном счёте (SEC).
//!
//! Сценарий: N реплик независимо выполняют случайные локальные операции,
//! затем происходит полный обмен состояниями, причём каждая реплика
//! сливает чужие состояния **в своём порядке** (циклический сдвиг) и с
//! **повторной доставкой** одного из состояний. Если после этого все
//! реплики равны — на данном прогоне продемонстрирована независимость
//! результата от порядка и кратности доставки, т.е. рабочее следствие
//! законов ACI.
//!
//! Инфляционность локальных операций (второе условие теоремы) проверяется
//! обобщённо в `tests/laws.rs` (макрос `inflation_law!`).

mod common;

use common::ActorId;
use crdt_lab::orset::Op as SetOp;
use crdt_lab::{CvRdt, GCounter, OrSet, Orswot, PnCounter, Update};
use proptest::prelude::*;

/// Полный обмен: реплика `i` сливает остальных начиная с `i+1`-й
/// (свой порядок у каждой), а состояние соседа доставляется дважды.
fn gossip_everyone<T: CvRdt>(replicas: &mut [T]) {
    let snapshot: Vec<T> = replicas.to_vec();
    let n = snapshot.len();
    for (i, replica) in replicas.iter_mut().enumerate() {
        for k in 1..n {
            replica.merge(&snapshot[(i + k) % n]);
        }
        replica.merge(&snapshot[(i + 1) % n]); // повторная доставка
    }
}

fn assert_all_equal<T: CvRdt + std::fmt::Debug>(replicas: &[T]) {
    for pair in replicas.windows(2) {
        assert_eq!(pair[0], pair[1], "реплики не сошлись");
    }
}

proptest! {
    /// SEC для G-Counter: значение после сходимости — сумма всех операций.
    #[test]
    fn gcounter_replicas_converge(
        ops in proptest::collection::vec((0..3usize, 1..10u64), 0..20)
    ) {
        let mut replicas = vec![GCounter::new(); 3];
        let mut total = 0u64;
        for (replica, amount) in ops {
            // актор = номер реплики: каждый пишет только свою компоненту
            replicas[replica].increment_by(replica as u8, amount);
            total += amount;
        }

        gossip_everyone(&mut replicas);

        assert_all_equal(&replicas);
        prop_assert_eq!(replicas[0].value(), total);
    }

    /// SEC для PN-Counter: значение — алгебраическая сумма всех операций.
    #[test]
    fn pncounter_replicas_converge(
        ops in proptest::collection::vec((0..3usize, proptest::bool::ANY), 0..20)
    ) {
        let mut replicas = vec![PnCounter::new(); 3];
        let mut total = 0i64;
        for (replica, positive) in ops {
            if positive {
                replicas[replica].increment(replica as u8);
                total += 1;
            } else {
                replicas[replica].decrement(replica as u8);
                total -= 1;
            }
        }

        gossip_everyone(&mut replicas);

        assert_all_equal(&replicas);
        prop_assert_eq!(replicas[0].value(), total);
    }

    /// SEC для OR-Set: реплики сходятся к одному наблюдаемому множеству.
    #[test]
    fn orset_replicas_converge(
        ops in proptest::collection::vec((0..3usize, common::orset_op()), 0..24)
    ) {
        let mut replicas: Vec<OrSet<ActorId, u8>> = vec![OrSet::new(); 3];
        for (replica, op) in ops {
            // метки делаем заведомо уникальными: актор = номер реплики
            let op = match op {
                SetOp::Add(_, item) => SetOp::Add(replica as u8, item),
                remove => remove,
            };
            replicas[replica].apply(op);
        }

        gossip_everyone(&mut replicas);
        assert_all_equal(&replicas);
    }

    /// SEC для ORSWOT: сходимость несмотря на нетривиальный `join`
    /// (метки могут умирать при слиянии) и повторные доставки.
    #[test]
    fn orswot_replicas_converge(
        ops in proptest::collection::vec((0..3usize, common::orset_op()), 0..24)
    ) {
        let mut replicas: Vec<Orswot<ActorId, u8>> = vec![Orswot::new(); 3];
        for (replica, op) in ops {
            // метки делаем заведомо уникальными: актор = номер реплики
            let op = match op {
                SetOp::Add(_, item) => SetOp::Add(replica as u8, item),
                remove => remove,
            };
            replicas[replica].apply(op);
        }

        gossip_everyone(&mut replicas);
        assert_all_equal(&replicas);
    }

    /// Слияние после слияния ничего не меняет: достигнута неподвижная точка.
    #[test]
    fn merge_is_stable_after_convergence(
        ops in proptest::collection::vec((0..3usize, common::orset_op()), 0..24)
    ) {
        let mut replicas: Vec<OrSet<ActorId, u8>> = vec![OrSet::new(); 3];
        for (replica, op) in ops {
            replicas[replica].apply(op);
        }
        gossip_everyone(&mut replicas);
        let converged = replicas.clone();
        gossip_everyone(&mut replicas);
        prop_assert_eq!(replicas, converged);
    }
}
