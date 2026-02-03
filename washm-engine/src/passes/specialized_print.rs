use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::{ExportKind, Module};
use crate::ir::visitor::ReadOnlyVisitor;
use crate::ir::FuncId;
use crate::passes::Pass;
use std::collections::HashSet;

/// Prints the call graph of the module.
pub struct PrintCallGraph;

impl Pass for PrintCallGraph {
    fn name(&self) -> &str {
        "PrintCallGraph"
    }

    fn run(&mut self, module: &mut Module) {
        println!("Call Graph:");
        for (_, func) in &module.functions {
            let mut collector = CallCollector::default();
            if let Some(body) = func.body() {
                collector.visit(body, &module.expressions);
            }

            let mut targets: Vec<_> = collector
                .targets
                .into_iter()
                .map(|id| module.functions[id].name.clone())
                .collect();
            targets.sort();

            print!("  {} ->", func.name);
            if targets.is_empty() {
                println!(" (none)");
            } else {
                for target in targets {
                    print!(" {}", target);
                }
                println!();
            }
        }
    }
}

#[derive(Default)]
struct CallCollector {
    targets: HashSet<FuncId>,
}

impl ReadOnlyVisitor for CallCollector {
    fn visit_expression(
        &mut self,
        expr_id: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        if let ExpressionKind::Call { target, .. } = &arena[expr_id].kind {
            self.targets.insert(*target);
        }
    }
}

/// Prints a map of functions and their exports.
pub struct PrintFunctionMap;

impl Pass for PrintFunctionMap {
    fn name(&self) -> &str {
        "PrintFunctionMap"
    }

    fn run(&mut self, module: &mut Module) {
        println!("Function Map:");
        for (id, func) in &module.functions {
            let mut exports = Vec::new();
            for export in &module.exports {
                if let ExportKind::Function(export_id) = export.kind {
                    if export_id == id {
                        exports.push(&export.name);
                    }
                }
            }

            print!("  [{:?}] {}", id, func.name);
            if !exports.is_empty() {
                print!(
                    " (exports: {})",
                    exports
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            println!();
        }
    }
}

/// Prints the module in a minified format (no spaces, newlines).
pub struct PrintMinified;

impl Pass for PrintMinified {
    fn name(&self) -> &str {
        "PrintMinified"
    }

    fn run(&mut self, _module: &mut Module) {
        // This would use a dedicated printer. For now, we reuse the existing printer
        // but with a flag if it supported it.
        println!("Minified Output: [not implemented]");
    }
}
