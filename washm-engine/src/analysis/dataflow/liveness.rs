use crate::ir::expression::ExprRef;
use crate::ir::LocalId;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Get,
    Set,
    Other,
}

#[derive(Debug, Clone)]
pub struct LivenessAction<'a> {
    pub kind: ActionKind,
    pub id: LocalId,
    pub effective: bool,
    pub origin: ExprRef<'a>,
    pub copy_from: Option<LocalId>,
}

impl<'a> LivenessAction<'a> {
    pub fn get(id: LocalId, origin: ExprRef<'a>) -> Self {
        Self {
            kind: ActionKind::Get,
            id,
            effective: false,
            origin,
            copy_from: None,
        }
    }
    pub fn set(id: LocalId, origin: ExprRef<'a>, copy_from: Option<LocalId>) -> Self {
        Self {
            kind: ActionKind::Set,
            id,
            effective: false,
            origin,
            copy_from,
        }
    }

    pub fn is_get(&self) -> bool {
        self.kind == ActionKind::Get
    }
    pub fn is_set(&self) -> bool {
        self.kind == ActionKind::Set
    }
}

#[derive(Debug, Clone, Default)]
pub struct Liveness<'a> {
    pub start: HashSet<LocalId>,
    pub end: HashSet<LocalId>,
    pub actions: Vec<LivenessAction<'a>>,
}

pub struct InterferenceGraph {
    pub matrix: HashMap<LocalId, HashSet<LocalId>>,
}

impl Default for InterferenceGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl InterferenceGraph {
    pub fn new() -> Self {
        Self {
            matrix: HashMap::new(),
        }
    }

    pub fn add(&mut self, a: LocalId, b: LocalId) {
        if a == b {
            return;
        }
        let (min, max) = if a < b { (a, b) } else { (b, a) };
        self.matrix.entry(min).or_default().insert(max);
    }

    pub fn interferes(&self, a: LocalId, b: LocalId) -> bool {
        if a == b {
            return true;
        }
        let (min, max) = if a < b { (a, b) } else { (b, a) };
        self.matrix
            .get(&min)
            .map(|s| s.contains(&max))
            .unwrap_or(false)
    }
}
