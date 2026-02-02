use crate::ir::expression::ExprRef;
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
    pub index: u32,
    pub effective: bool,
    pub origin: ExprRef<'a>,
    pub copy_from: Option<u32>,
}

impl<'a> LivenessAction<'a> {
    pub fn get(index: u32, origin: ExprRef<'a>) -> Self {
        Self {
            kind: ActionKind::Get,
            index,
            effective: false,
            origin,
            copy_from: None,
        }
    }
    pub fn set(index: u32, origin: ExprRef<'a>, copy_from: Option<u32>) -> Self {
        Self {
            kind: ActionKind::Set,
            index,
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
    pub start: HashSet<u32>,
    pub end: HashSet<u32>,
    pub actions: Vec<LivenessAction<'a>>,
}

pub struct InterferenceGraph {
    pub matrix: HashMap<u32, HashSet<u32>>,
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

    pub fn add(&mut self, a: u32, b: u32) {
        if a == b {
            return;
        }
        let (min, max) = if a < b { (a, b) } else { (b, a) };
        self.matrix.entry(min).or_default().insert(max);
    }

    pub fn interferes(&self, a: u32, b: u32) -> bool {
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
