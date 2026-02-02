use crate::analysis::cfg::{BlockId, ControlFlowGraph};
use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::{Function, LocalId};
use crate::ir::visitor::ReadOnlyVisitor;
use std::collections::{HashMap, HashSet};

pub struct Liveness<'a> {
    pub cfg: ControlFlowGraph<'a>,
    // BasicBlock ID -> Set of Live Locals at Entry
    pub live_in: HashMap<BlockId, HashSet<LocalId>>,
    // BasicBlock ID -> Set of Live Locals at Exit
    pub live_out: HashMap<BlockId, HashSet<LocalId>>,
}

impl<'a> Liveness<'a> {
    pub fn new(func: &Function<'a>) -> Self {
        if func.body().is_none() {
            // We need a dummy CFG or optional
            // For simplicity, panic or handle. Empty func has no body.
            // But we can't create CFG without entry expr.
            // let's return empty if no body
            panic!("Liveness requires function body");
        }

        let body = func.body().unwrap();
        let cfg = ControlFlowGraph::build(func, body);

        // 2. Compute Gen definitions
        let mut gen: HashMap<BlockId, HashSet<LocalId>> = HashMap::new();
        let mut kill: HashMap<BlockId, HashSet<LocalId>> = HashMap::new();

        for (id, block) in cfg.blocks.iter() {
            let mut g = HashSet::new();
            let mut k = HashSet::new();

            // Scan block contents (expressions)
            // Note: CFG contents are typically just top-level expressions in the block.
            // We need to deep visit them.
            for &expr in &block.contents {
                let mut scanner = LivenessScanner {
                    gen: &mut g,
                    kill: &mut k,
                };
                scanner.visit(expr);
            }

            gen.insert(id, g);
            kill.insert(id, k);
        }

        // 3. Iterative solver
        let mut live_in: HashMap<BlockId, HashSet<LocalId>> = HashMap::new();
        let mut live_out: HashMap<BlockId, HashSet<LocalId>> = HashMap::new();

        // Initialize
        for (id, _) in cfg.blocks.iter() {
            live_in.insert(id, HashSet::new());
            live_out.insert(id, HashSet::new());
        }

        let mut changed = true;
        while changed {
            changed = false;

            // Visit blocks in whatever order (reverse post-order is best, but any works for convergence)
            for (id, block) in cfg.blocks.iter() {
                // Calculate LiveOut = Union(LiveIn(succ))
                let mut current_live_out = HashSet::new();
                for &succ_id in &block.succs {
                    if let Some(succ_in) = live_in.get(&succ_id) {
                        for &local in succ_in {
                            current_live_out.insert(local);
                        }
                    }
                }

                // Calculate LiveIn = Gen U (LiveOut - Kill)
                let mut current_live_in = gen.get(&id).unwrap().clone();
                for &local in &current_live_out {
                    if !kill.get(&id).unwrap().contains(&local) {
                        current_live_in.insert(local);
                    }
                }

                if current_live_in != *live_in.get(&id).unwrap() {
                    live_in.insert(id, current_live_in);
                    changed = true;
                }
                if current_live_out != *live_out.get(&id).unwrap() {
                    live_out.insert(id, current_live_out);
                    changed = true;
                }
            }
        }

        Self {
            cfg,
            live_in,
            live_out,
        }
    }
}

struct LivenessScanner<'a> {
    gen: &'a mut HashSet<LocalId>,
    kill: &'a mut HashSet<LocalId>,
}

impl<'a> ReadOnlyVisitor<'a> for LivenessScanner<'a> {
    fn visit_expression(&mut self, expr: ExprRef<'a>) {
        match &expr.kind {
            ExpressionKind::LocalGet { id } => {
                if !self.kill.contains(id) {
                    self.gen.insert(*id);
                }
            }
            ExpressionKind::LocalSet { id, value } => {
                self.visit(*value); // Visit value first (it is read)
                self.kill.insert(*id);
            }
            ExpressionKind::LocalTee { id, value } => {
                self.visit(*value);
                self.kill.insert(*id);
                // Generally Tee is "Set local, then result is value".
                // So it kills.
            }
            _ => {
                self.visit_children(expr);
            }
        }
    }
}
