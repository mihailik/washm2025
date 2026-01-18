use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::{ExportKind, Module};
use crate::ir::visitor::ReadOnlyVisitor;
use crate::passes::Pass;
use std::collections::HashSet;

/// Prints the call graph of the module.
pub struct PrintCallGraph;

impl Pass for PrintCallGraph {
    fn name(&self) -> &str {
        "PrintCallGraph"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        println!("Call Graph:");
        for func in &module.functions {
            let mut collector = CallCollector::default();
            if let Some(body) = func.body {
                collector.visit(body);
            }

            let mut targets: Vec<_> = collector.targets.into_iter().collect();
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
    targets: HashSet<String>,
}

impl<'a> ReadOnlyVisitor<'a> for CallCollector {
    fn visit_expression(&mut self, expr: ExprRef<'a>) {
        if let ExpressionKind::Call { target, .. } = &expr.kind {
            self.targets.insert(target.to_string());
        }
    }
}

/// Prints a map of functions and their exports.
pub struct PrintFunctionMap;

impl Pass for PrintFunctionMap {
    fn name(&self) -> &str {
        "PrintFunctionMap"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        println!("Function Map:");
        for (i, func) in module.functions.iter().enumerate() {
            let mut exports = Vec::new();
            for export in &module.exports {
                if export.kind == ExportKind::Function
                    && export.index == (module.imports.len() + i) as u32
                {
                    exports.push(&export.name);
                }
            }

            print!("  [{}] {}", i, func.name);
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

    fn run<'a>(&mut self, _module: &mut Module<'a>) {
        // This would use a dedicated printer. For now, we reuse the existing printer
        // but with a flag if it supported it.
        println!("Minified Output: [not implemented]");
    }
}
