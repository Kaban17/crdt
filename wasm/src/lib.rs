//! WebAssembly-биндинги: демонстрация `web/index.html` исполняет
//! **настоящий** код `crdt-lab`, а не JS-порт его семантики.
//!
//! Экспортируется один объект [`Playground`] с тремя репликами (A, B, C)
//! всех шести типов. Наружу отдаются только операции публичного API
//! библиотеки и JSON-снимки наблюдаемого состояния; вся презентация
//! остаётся на стороне страницы.

use crdt_lab::{
    orset, GCounter, GSet, JoinSemilattice, LwwRegister, MvRegister, OrSet, PnCounter, Update,
};
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

const REPLICAS: [&str; 3] = ["A", "B", "C"];

/// Состояние одной реплики: по экземпляру каждого типа.
struct Node {
    gcounter: GCounter<String>,
    pncounter: PnCounter<String>,
    gset: GSet<String>,
    lww: LwwRegister<String, String>,
    mvreg: MvRegister<String, String>,
    orset: OrSet<String, String>,
}

impl Node {
    fn new() -> Self {
        Self {
            gcounter: GCounter::new(),
            pncounter: PnCounter::new(),
            gset: GSet::new(),
            // пустой актор и значение — "ещё не установлено"
            lww: LwwRegister::new(String::new(), String::new()),
            mvreg: MvRegister::new(),
            orset: OrSet::new(),
        }
    }
}

/// Три реплики и операции над ними — интерфейс демонстрации.
#[wasm_bindgen]
pub struct Playground {
    nodes: Vec<Node>,
}

fn index_of(replica: &str) -> usize {
    REPLICAS
        .iter()
        .position(|r| *r == replica)
        .expect("replica must be A, B or C")
}

#[wasm_bindgen]
impl Playground {
    /// Создать площадку с пустыми репликами.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Playground {
        Playground {
            nodes: (0..REPLICAS.len()).map(|_| Node::new()).collect(),
        }
    }

    /// Сбросить все реплики в начальное состояние.
    pub fn reset(&mut self) {
        self.nodes = (0..REPLICAS.len()).map(|_| Node::new()).collect();
    }

    // --- Локальные операции (crdt_lab::Update::apply под капотом) ---

    pub fn gcounter_increment(&mut self, replica: &str) {
        self.nodes[index_of(replica)]
            .gcounter
            .apply((replica.to_string(), 1));
    }

    pub fn pncounter_update(&mut self, replica: &str, positive: bool) {
        use crdt_lab::pncounter::Op;
        let op = if positive {
            Op::Inc(replica.to_string())
        } else {
            Op::Dec(replica.to_string())
        };
        self.nodes[index_of(replica)].pncounter.apply(op);
    }

    pub fn gset_add(&mut self, replica: &str, elem: &str) {
        self.nodes[index_of(replica)].gset.apply(elem.to_string());
    }

    pub fn lww_write(&mut self, replica: &str, timestamp: f64, value: &str) {
        self.nodes[index_of(replica)].lww.apply((
            timestamp as u64,
            replica.to_string(),
            value.to_string(),
        ));
    }

    pub fn mvreg_write(&mut self, replica: &str, value: &str) {
        self.nodes[index_of(replica)]
            .mvreg
            .apply((replica.to_string(), value.to_string()));
    }

    /// OR-Set: если элемент видим — удалить, иначе вставить.
    /// Возвращает строку для журнала операций.
    pub fn orset_toggle(&mut self, replica: &str, elem: &str) -> String {
        let set = &mut self.nodes[index_of(replica)].orset;
        if set.contains(&elem.to_string()) {
            set.apply(orset::Op::Remove(elem.to_string()));
            format!("remove(\"{elem}\")")
        } else {
            set.apply(orset::Op::Add(replica.to_string(), elem.to_string()));
            format!("add(\"{elem}\")")
        }
    }

    // --- Слияние и наблюдение ---

    /// Слить состояния данного типа между двумя репликами (`⊔` библиотеки,
    /// симметрично: обе получают точную верхнюю грань).
    pub fn sync(&mut self, kind: &str, x: &str, y: &str) {
        let (xi, yi) = (index_of(x), index_of(y));
        macro_rules! sync_field {
            ($field:ident) => {{
                let merged = self.nodes[xi].$field.clone().joined(&self.nodes[yi].$field);
                self.nodes[xi].$field = merged.clone();
                self.nodes[yi].$field = merged;
            }};
        }
        match kind {
            "gcounter" => sync_field!(gcounter),
            "pncounter" => sync_field!(pncounter),
            "gset" => sync_field!(gset),
            "lww" => sync_field!(lww),
            "mvreg" => sync_field!(mvreg),
            "orset" => sync_field!(orset),
            _ => {}
        }
    }

    /// Сошлись ли все реплики по данному типу (структурное `Eq` библиотеки).
    pub fn converged(&self, kind: &str) -> bool {
        macro_rules! all_eq {
            ($field:ident) => {
                self.nodes.windows(2).all(|w| w[0].$field == w[1].$field)
            };
        }
        match kind {
            "gcounter" => all_eq!(gcounter),
            "pncounter" => all_eq!(pncounter),
            "gset" => all_eq!(gset),
            "lww" => all_eq!(lww),
            "mvreg" => all_eq!(mvreg),
            "orset" => all_eq!(orset),
            _ => false,
        }
    }

    /// JSON-снимок наблюдаемого состояния типа по всем репликам.
    pub fn snapshot(&self, kind: &str) -> String {
        let per_replica: Vec<Value> = self.nodes.iter().map(|n| node_view(kind, n)).collect();
        json!(per_replica).to_string()
    }
}

impl Default for Playground {
    fn default() -> Self {
        Self::new()
    }
}

fn counts_view(c: &GCounter<String>) -> Value {
    Value::Object(
        REPLICAS
            .iter()
            .map(|r| ((*r).to_string(), json!(c.get(&(*r).to_string()))))
            .collect(),
    )
}

fn clock_view(clock: &crdt_lab::VClock<String>) -> Value {
    Value::Object(
        REPLICAS
            .iter()
            .map(|r| ((*r).to_string(), json!(clock.get(&(*r).to_string()))))
            .collect(),
    )
}

fn node_view(kind: &str, n: &Node) -> Value {
    match kind {
        "gcounter" => json!({
            "sum": n.gcounter.value(),
            "counts": counts_view(&n.gcounter),
        }),
        "pncounter" => json!({
            "value": n.pncounter.value(),
            "p": counts_view(n.pncounter.plus()),
            "n": counts_view(n.pncounter.minus()),
        }),
        "gset" => json!({
            "elements": n.gset.iter().collect::<Vec<_>>(),
        }),
        "lww" => json!({
            "value": n.lww.value(),
            "ts": n.lww.timestamp(),
            "actor": n.lww.actor(),
        }),
        "mvreg" => json!({
            "entries": n.mvreg
                .read_with_clocks()
                .map(|(clock, value)| json!({ "value": value, "clock": clock_view(clock) }))
                .collect::<Vec<_>>(),
        }),
        "orset" => json!({
            "visible": n.orset.iter().collect::<Vec<_>>(),
            "tombstones": n.orset.tombstone_count(),
        }),
        _ => Value::Null,
    }
}
