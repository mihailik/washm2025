use crate::ir::module::Module;
use crate::passes::Pass;

pub struct Memory64Lowering;

impl Pass for Memory64Lowering {
    fn name(&self) -> &str {
        "memory64-lowering"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        // Memory64Lowering
        // Goal: Convert 64-bit memory access to 32-bit (wrapping/bounds checking).

        // This requires visiting all memory instructions (load/store/memory.copy etc.)
        // and injecting logic to handle 64-bit offsets/indices.

        // Check if module uses memory64.
        let is_memory64 = if let Some(_) = module.memories.ids().next() {
            // Wait, MemoryLimits doesn't store index type (i32/i64).
            // Binaryen-ir Module might assume i32 memory for MVP, but Memory64 proposal adds i64.
            // Currently our MemoryLimits struct in module.rs:
            // pub struct MemoryLimits { pub initial: u32, pub maximum: Option<u32> }
            // So we only support 32-bit memory in our IR definition currently!

            // If the IR doesn't support 64-bit memory yet, this pass is effectively a no-op
            // or placeholder for when we add Memory64 support.
            false
        } else {
            false
        };

        if !is_memory64 {
            return;
        }

        for (_, func) in &mut module.functions {
            if let Some(_body) = func.body() {
                // TODO: lowering logic
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
    use crate::ir::module::Function;
    use crate::ir::FuncId;
    use crate::ir::Type;
    use bumpalo::collections::Vec as BumpVec;
    use bumpalo::Bump;

    #[test]
    fn test_memory64_lowering_run() {
        let allocator = Bump::new();
        let mut module = Module::new(&allocator);

        let block = allocator.alloc(Expression {
            kind: ExpressionKind::Block {
                name: None,
                list: BumpVec::new_in(&allocator),
            },
            type_: Type::NONE,
        });

        let func = Function::new(
            "test_func".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(ExprRef::new(block)),
        );
        module.add_function(func);

        let mut pass = Memory64Lowering;
        pass.run(&mut module);

        assert!(module.get_function(FuncId::from_raw_parts(0, 0)).is_some());
    }
}
