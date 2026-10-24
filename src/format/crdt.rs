//! CRDT building blocks of the scene format: item ids, last-writer-wins registers and the
//! sequences whose items name their left and right neighbors.

use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct CrdtId(pub u8, pub u64);

impl std::fmt::Display for CrdtId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.0, self.1)
    }
}

/// Item id meaning "no neighbor" on the left/right of a CRDT sequence item.
pub const END_MARKER: CrdtId = CrdtId(0, 0);
/// Root group of the scene tree; its children are the layers.
pub const ROOT_ID: CrdtId = CrdtId(0, 1);

/// Last-writer-wins register.
#[derive(Debug, Clone, PartialEq)]
pub struct Lww<T> {
    pub timestamp: CrdtId,
    pub value: T,
}

/// One entry of a CRDT sequence (group children or text characters).
#[derive(Debug, Clone, PartialEq)]
pub struct Item<V> {
    pub id: CrdtId,
    pub left: CrdtId,
    pub right: CrdtId,
    pub deleted_length: u32,
    pub value: Option<V>,
}

/// Order a CRDT sequence: each item sits between its `left` and `right` neighbors.
/// Items that are unordered relative to each other come out sorted by id.
pub fn toposort<V>(items: &[Item<V>]) -> Vec<CrdtId> {
    let ids: HashSet<CrdtId> = items.iter().map(|i| i.id).collect();
    // deps[x] = set of items that must come before x
    let mut deps: BTreeMap<CrdtId, HashSet<CrdtId>> =
        items.iter().map(|i| (i.id, HashSet::new())).collect();
    for it in items {
        if ids.contains(&it.left) {
            deps.get_mut(&it.id).unwrap().insert(it.left);
        }
        if ids.contains(&it.right) {
            deps.get_mut(&it.right).unwrap().insert(it.id);
        }
    }
    let mut out = Vec::with_capacity(items.len());
    while !deps.is_empty() {
        let ready: Vec<CrdtId> = deps
            .iter()
            .filter(|(_, d)| d.is_empty())
            .map(|(k, _)| *k)
            .collect();
        if ready.is_empty() {
            // cycle: emit the rest by id rather than lose items
            out.extend(deps.keys().copied());
            break;
        }
        for k in &ready {
            deps.remove(k);
        }
        for d in deps.values_mut() {
            for k in &ready {
                d.remove(k);
            }
        }
        out.extend(ready);
    }
    out
}
