# crdt-lab

Кодовая база к научной работе **«Реализация и анализ свойств сходимости CRDT
средствами системы типов Rust»**.

## Идея

Состояние-ориентированный CRDT (CvRDT) — это тип, состояния которого образуют
полурешётку соединения, локальные операции инфляционны, а слияние реплик —
операция `⊔`. Из этого следует сильная согласованность в конечном счёте (SEC,
Shapiro et al., 2011).

Работа исследует, какую часть этой спецификации можно «переложить» на систему
типов Rust:

- **Трейт как алгебраическая сигнатура.** `JoinSemilattice` (`src/lattice.rs`)
  фиксирует операцию `join`; порядок `≤` не пишется вручную, а *выводится* из
  неё (`a ≤ b ⟺ a ⊔ b = b`), поэтому рассогласовать порядок и слияние
  невозможно по построению.
- **Blanket impl как теорема.** `CvRdt` реализован автоматически для любой
  полурешётки: соответствие «полурешётка ⇒ CvRDT» закреплено на уровне типов.
- **Композиция решёток.** `PnCounter` — произведение двух `GCounter`,
  `OrSet` — произведение трёх растущих структур; законы наследуются
  покомпонентно.
- **Законы — property-based тестами.** То, что система типов Rust выразить не
  может (сами уравнения), проверяется обобщёнными proptest-тестами, написанными
  один раз для любого `T: JoinSemilattice` и инстанцируемыми макросом
  (`tests/laws.rs`). Новый CRDT подключается к проверке одной строкой.

## Структура

| Путь | Содержимое |
|---|---|
| `src/lattice.rs` | `JoinSemilattice`, `CvRdt`, `Actor` — ядро |
| `src/vclock.rs` | векторные часы, `PartialOrd` = «произошло-до» |
| `src/gcounter.rs` | растущий счётчик |
| `src/pncounter.rs` | счётчик с декрементом (произведение решёток) |
| `src/gset.rs` | растущее множество |
| `src/lww.rs` | LWW-регистр (лексикографический максимум) |
| `src/orset.rs` | OR-Set с надгробиями, семантика add-wins |
| `tests/common/mod.rs` | proptest-стратегии: состояния через операции API |
| `tests/laws.rs` | обобщённые законы полурешётки для всех типов |
| `tests/convergence.rs` | имитация реплик: SEC, инфляционность, неподвижная точка |

## Команды

```sh
cargo test              # все тесты (юнит + property-based + doc-тесты)
cargo doc --open        # документация с теоретическими комментариями
cargo clippy --all-targets
```

## Дальнейшая работа

- OR-Set без надгробий (причинный контекст / dotted version vectors);
- операционные CRDT (CmRDT) и сравнение гарантий двух моделей;
- delta-CRDT: `join` над дельтами вместо полных состояний;
- формализация законов вне тестов (Kani/Creusot) как усиление тезиса.

## Литература

- M. Shapiro, N. Preguiça, C. Baquero, M. Zawirski. *Conflict-free Replicated
  Data Types.* SSS 2011.
- M. Shapiro et al. *A comprehensive study of Convergent and Commutative
  Replicated Data Types.* INRIA RR-7506, 2011.
- C. Baquero, P. S. Almeida, A. Shoker. *Making Operation-based CRDTs
  Operation-based.* DAIS 2014.
- P. S. Almeida, A. Shoker, C. Baquero. *Delta State Replicated Data Types.*
  JPDC 2018.
