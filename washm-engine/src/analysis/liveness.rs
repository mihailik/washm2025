use crate::analysis::cfg::{BasicBlockA, ControlFlowGraph};
use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Function;
use crate::ir::visitor::ReadOnlyVisitor;
use std::collections::{HashMap, HashSet};

pub struct Liveness<'a> {
    pub cfg: ControlFlowGraph<'a>,
    // BasicBlock ID -> Set of Live Locals at Entry
    pub live_in: HashMap<u32, HashSet<u32>>,
    // BasicBlock ID -> Set of Live Locals at Exit
    pub live_out: HashMap<u32, HashSet<u32>>,
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
        let mut gen: HashMap<u32, HashSet<u32>> = HashMap::new();
        let mut kill: HashMap<u32, HashSet<u32>> = HashMap::new();

        for block in &cfg.blocks {
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

            gen.insert(block.id, g);
            kill.insert(block.id, k);
        }

        // 3. Iterative solver
        let mut live_in: HashMap<u32, HashSet<u32>> = HashMap::new();
        let mut live_out: HashMap<u32, HashSet<u32>> = HashMap::new();

        // Initialize
        for block in &cfg.blocks {
            live_in.insert(block.id, HashSet::new());
            live_out.insert(block.id, HashSet::new());
        }

        let mut changed = true;
        while changed {
            changed = false;

            // Visit blocks in whatever order (reverse post-order is best, but any works for convergence)
            for block in &cfg.blocks {
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
                let mut current_live_in = gen[&block.id].clone();
                for &local in &current_live_out {
                    if !kill[&block.id].contains(&local) {
                        current_live_in.insert(local);
                    }
                }

                if current_live_in != live_in[&block.id] {
                    live_in.insert(block.id, current_live_in);
                    changed = true;
                }
                if current_live_out != live_out[&block.id] {
                    live_out.insert(block.id, current_live_out);
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
    gen: &'a mut HashSet<u32>,
    kill: &'a mut HashSet<u32>,
}

impl<'a> ReadOnlyVisitor<'a> for LivenessScanner<'a> {
    fn visit_expression(&mut self, expr: ExprRef<'a>) {
        match &expr.kind {
            ExpressionKind::LocalGet { index } => {
                if !self.kill.contains(index) {
                    self.gen.insert(*index);
                }
            }
            ExpressionKind::LocalSet { index, value } => {
                self.visit(*value); // Visit value first (it is read)
                self.kill.insert(*index);
            }
            ExpressionKind::LocalTee { index, value } => {
                self.visit(*value);
                self.kill.insert(*index);
                // Generally Tee is "Set local, then result is value".
                // So it kills.
            }
            _ => {
                self.visit_children(expr);
            }
        }
    }
}
