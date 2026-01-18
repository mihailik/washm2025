use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Function;
use crate::ir::visitor::ReadOnlyVisitor;

/// Cost Estimator for inlining decisions
pub struct CostEstimator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrivialInstruction {
    /// Function is not a single instruction, or it may not shrink when inlined.
    NotTrivial,

    /// Function is just one instruction, with `local.get`s as arguments, and with
    /// each `local` is used exactly once, and in the order they appear in the
    /// argument list.
    Shrinks,

    /// Function is a single instruction, but maybe with constant arguments, or
    /// maybe some locals are used more than once.
    MayNotShrink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cost {
    pub instruction_count: u32,
    pub call_count: u32,
    pub loop_count: u32,
    pub has_try_delegate: bool,
    pub trivial_instruction: TrivialInstruction,
}

impl CostEstimator {
    /// Estimate cost of inlining this function
    pub fn inline_cost(func: &Function) -> Cost {
        let mut calculator = CostCalculator::new(func);
        if let Some(body) = &func.body {
            calculator.visit(*body);
            calculator.analyze_trivial(*body);
        }
        calculator.cost
    }
}

struct CostCalculator {
    cost: Cost,
    param_count: usize,
}

impl CostCalculator {
    fn new(func: &Function) -> Self {
        Self {
            cost: Cost {
                instruction_count: 0,
                call_count: 0,
                loop_count: 0,
                has_try_delegate: false,
                trivial_instruction: TrivialInstruction::NotTrivial,
            },
            param_count: func.params.tuple_len(),
        }
    }

    fn analyze_trivial<'a>(&mut self, body: ExprRef<'a>) {
        // A function is trivial if its body is a single instruction.
        // If it's a block with one instruction, that's also trivial.
        let mut current = body;

        if let ExpressionKind::Block { list, .. } = &current.kind {
            if list.len() == 1 {
                current = list[0];
            } else if list.is_empty() {
                // Empty block is also trivial (shrinks)
                self.cost.trivial_instruction = TrivialInstruction::Shrinks;
                return;
            } else {
                return;
            }
        }

        match &current.kind {
            ExpressionKind::Block { .. }
            | ExpressionKind::Loop { .. }
            | ExpressionKind::If { .. }
            | ExpressionKind::Try { .. } => {
                // Control flow is not trivial in this sense
                return;
            }
            ExpressionKind::LocalGet { .. }
            | ExpressionKind::Const(_)
            | ExpressionKind::Unreachable => {
                self.cost.trivial_instruction = TrivialInstruction::Shrinks;
                return;
            }
            _ => {}
        }

        // Check operands if it's a simple instruction
        let mut operands = Vec::new();
        match &current.kind {
            ExpressionKind::Unary { value, .. } => operands.push(*value),
            ExpressionKind::Binary { left, right, .. } => {
                operands.push(*left);
                operands.push(*right);
            }
            ExpressionKind::Select {
                if_true,
                if_false,
                condition,
            } => {
                operands.push(*if_true);
                operands.push(*if_false);
                operands.push(*condition);
            }
            ExpressionKind::Call {
                operands: ops,
                is_return,
                ..
            } if !*is_return => {
                for op in ops.iter() {
                    operands.push(*op);
                }
            }
            _ => {
                // For other complex instructions, we don't consider them trivial for now
                return;
            }
        }

        if operands.len() != self.param_count {
            self.cost.trivial_instruction = TrivialInstruction::MayNotShrink;
            return;
        }

        let mut shrink = true;
        for (i, op) in operands.iter().enumerate() {
            if let ExpressionKind::LocalGet { index } = &op.kind {
                if *index != i as u32 {
                    shrink = false;
                    break;
                }
            } else {
                shrink = false;
                break;
            }
        }

        if shrink {
            self.cost.trivial_instruction = TrivialInstruction::Shrinks;
        } else {
            self.cost.trivial_instruction = TrivialInstruction::MayNotShrink;
        }
    }
}

impl<'a> ReadOnlyVisitor<'a> for CostCalculator {
    fn visit_expression(&mut self, expr: ExprRef<'a>) {
        self.cost.instruction_count += 1;

        match &expr.kind {
            ExpressionKind::Call { .. } | ExpressionKind::CallIndirect { .. } => {
                self.cost.call_count += 1;
            }
            ExpressionKind::Loop { .. } => {
                self.cost.loop_count += 1;
            }
            ExpressionKind::Try { delegate, .. } => {
                if delegate.is_some() {
                    self.cost.has_try_delegate = true;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::Type;
    use bumpalo::Bump;

    #[test]
    fn test_cost_estimation() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // (block
        //   (nop)
        //   (loop (nop))
        //   (call "foo")
        // )
        // Count: Block(1) + Nop(1) + Loop(1) + Nop(1) + Call(1) = 5 instrs
        // Loop count: 1
        // Call count: 1

        let nop1 = builder.nop();
        let nop2 = builder.nop();
        let loop_ = builder.loop_(None, nop2, Type::NONE);
        let call = builder.call(
            "foo",
            bumpalo::collections::Vec::new_in(&bump),
            Type::NONE,
            false,
        );

        let mut list = bumpalo::collections::Vec::new_in(&bump);
        list.push(nop1);
        list.push(loop_);
        list.push(call);

        let block = builder.block(None, list, Type::NONE);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(block),
        );

        let cost = CostEstimator::inline_cost(&func);

        assert_eq!(cost.instruction_count, 5);
        assert_eq!(cost.loop_count, 1);
        assert_eq!(cost.call_count, 1);
    }
}
