use crate::analysis::dataflow::cfg::CFGBuilder;
use crate::analysis::dataflow::liveness::InterferenceGraph;
use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::{Function, Module};
use crate::ir::visitor::Visitor;
use crate::ir::Type;
use crate::passes::Pass;

pub struct CoalesceLocals;

impl Pass for CoalesceLocals {
    fn name(&self) -> &str {
        "coalesce-locals"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        for (_, func) in &mut module.functions {
            Self::run_on_func(func);
        }
    }
}

struct LocalMapper<'a> {
    mapping: &'a mut [u32],
}

impl<'a, 'b> Visitor<'a> for LocalMapper<'b> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let Some(expr_mut) = unsafe { expr.as_ptr().as_mut() } {
            match &mut expr_mut.kind {
                ExpressionKind::LocalGet { index } => {
                    if let Some(&new_idx) = self.mapping.get(*index as usize) {
                        *index = new_idx;
                    }
                }
                ExpressionKind::LocalSet { index, .. } => {
                    if let Some(&new_idx) = self.mapping.get(*index as usize) {
                        *index = new_idx;
                    }
                }
                ExpressionKind::LocalTee { index, .. } => {
                    if let Some(&new_idx) = self.mapping.get(*index as usize) {
                        *index = new_idx;
                    }
                }
                _ => {}
            }
        }
    }
}

struct CopyCoalescingVisitor<'a> {
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a, 'b> Visitor<'a> for CopyCoalescingVisitor<'b> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        // Manually visit children for recursive traversal based on ExpressionKind variants
        if let Some(expr_mut) = unsafe { expr.as_ptr().as_mut() } {
            match &mut expr_mut.kind {
                ExpressionKind::Block { list, .. } => {
                    for e in list.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::If {
                    condition,
                    if_true,
                    if_false,
                } => {
                    self.visit_expression(condition);
                    self.visit_expression(if_true);
                    if let Some(f) = if_false {
                        self.visit_expression(f);
                    }
                }
                ExpressionKind::Loop { body, .. } => {
                    self.visit_expression(body);
                }
                ExpressionKind::Unary { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::Binary { left, right, .. } => {
                    self.visit_expression(left);
                    self.visit_expression(right);
                }
                ExpressionKind::Call { operands, .. } => {
                    for e in operands.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::LocalSet { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::LocalTee { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::GlobalSet { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::Break {
                    condition, value, ..
                } => {
                    if let Some(c) = condition {
                        self.visit_expression(c);
                    }
                    if let Some(v) = value {
                        self.visit_expression(v);
                    }
                }
                ExpressionKind::Return { value } => {
                    if let Some(v) = value {
                        self.visit_expression(v);
                    }
                }
                ExpressionKind::Drop { value } => {
                    self.visit_expression(value);
                }
                ExpressionKind::Select {
                    condition,
                    if_true,
                    if_false,
                } => {
                    self.visit_expression(condition);
                    self.visit_expression(if_true);
                    self.visit_expression(if_false);
                }
                ExpressionKind::Load { ptr, .. } => {
                    self.visit_expression(ptr);
                }
                ExpressionKind::Store { ptr, value, .. } => {
                    self.visit_expression(ptr);
                    self.visit_expression(value);
                }
                ExpressionKind::Switch {
                    condition, value, ..
                } => {
                    self.visit_expression(condition);
                    if let Some(v) = value {
                        self.visit_expression(v);
                    }
                }
                ExpressionKind::CallIndirect {
                    target, operands, ..
                } => {
                    self.visit_expression(target);
                    for e in operands.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::MemoryGrow { delta, .. } => {
                    self.visit_expression(delta);
                }
                ExpressionKind::AtomicRMW { ptr, value, .. } => {
                    self.visit_expression(ptr);
                    self.visit_expression(value);
                }
                ExpressionKind::AtomicCmpxchg {
                    ptr,
                    expected,
                    replacement,
                    ..
                } => {
                    self.visit_expression(ptr);
                    self.visit_expression(expected);
                    self.visit_expression(replacement);
                }
                ExpressionKind::AtomicWait {
                    ptr,
                    expected,
                    timeout,
                    ..
                } => {
                    self.visit_expression(ptr);
                    self.visit_expression(expected);
                    self.visit_expression(timeout);
                }
                ExpressionKind::AtomicNotify { ptr, count, .. } => {
                    self.visit_expression(ptr);
                    self.visit_expression(count);
                }
                ExpressionKind::TupleMake { operands } => {
                    for e in operands.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::TupleExtract { tuple, .. } => {
                    self.visit_expression(tuple);
                }
                ExpressionKind::RefIsNull { value } => {
                    self.visit_expression(value);
                }
                ExpressionKind::RefEq { left, right } => {
                    self.visit_expression(left);
                    self.visit_expression(right);
                }
                ExpressionKind::RefAs { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::TableGet { index, .. } => {
                    self.visit_expression(index);
                }
                ExpressionKind::TableSet { index, value, .. } => {
                    self.visit_expression(index);
                    self.visit_expression(value);
                }
                ExpressionKind::TableGrow { value, delta, .. } => {
                    self.visit_expression(value);
                    self.visit_expression(delta);
                }
                ExpressionKind::TableFill {
                    dest, value, size, ..
                } => {
                    self.visit_expression(dest);
                    self.visit_expression(value);
                    self.visit_expression(size);
                }
                ExpressionKind::TableCopy {
                    dest, src, size, ..
                } => {
                    self.visit_expression(dest);
                    self.visit_expression(src);
                    self.visit_expression(size);
                }
                ExpressionKind::TableInit {
                    dest, offset, size, ..
                } => {
                    self.visit_expression(dest);
                    self.visit_expression(offset);
                    self.visit_expression(size);
                }
                ExpressionKind::MemoryInit {
                    dest, offset, size, ..
                } => {
                    self.visit_expression(dest);
                    self.visit_expression(offset);
                    self.visit_expression(size);
                }
                ExpressionKind::MemoryCopy {
                    dest, src, size, ..
                } => {
                    self.visit_expression(dest);
                    self.visit_expression(src);
                    self.visit_expression(size);
                }
                ExpressionKind::MemoryFill {
                    dest, value, size, ..
                } => {
                    self.visit_expression(dest);
                    self.visit_expression(value);
                    self.visit_expression(size);
                }
                ExpressionKind::I31New { value } => {
                    self.visit_expression(value);
                }
                ExpressionKind::I31Get { i31, .. } => {
                    self.visit_expression(i31);
                }
                ExpressionKind::SIMDExtract { vec, .. } => {
                    self.visit_expression(vec);
                }
                ExpressionKind::SIMDReplace { vec, value, .. } => {
                    self.visit_expression(vec);
                    self.visit_expression(value);
                }
                ExpressionKind::SIMDShuffle { left, right, .. } => {
                    self.visit_expression(left);
                    self.visit_expression(right);
                }
                ExpressionKind::SIMDTernary { a, b, c, .. } => {
                    self.visit_expression(a);
                    self.visit_expression(b);
                    self.visit_expression(c);
                }
                ExpressionKind::SIMDShift { vec, shift, .. } => {
                    self.visit_expression(vec);
                    self.visit_expression(shift);
                }
                ExpressionKind::SIMDLoad { ptr, .. } => {
                    self.visit_expression(ptr);
                }
                ExpressionKind::SIMDLoadStoreLane { ptr, vec, .. } => {
                    self.visit_expression(ptr);
                    self.visit_expression(vec);
                }
                ExpressionKind::StructNew { operands, .. } => {
                    for e in operands.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::StructGet { ptr, .. } => {
                    self.visit_expression(ptr);
                }
                ExpressionKind::StructSet { ptr, value, .. } => {
                    self.visit_expression(ptr);
                    self.visit_expression(value);
                }
                ExpressionKind::ArrayNew { size, init, .. } => {
                    self.visit_expression(size);
                    if let Some(i) = init {
                        self.visit_expression(i);
                    }
                }
                ExpressionKind::ArrayGet { ptr, index, .. } => {
                    self.visit_expression(ptr);
                    self.visit_expression(index);
                }
                ExpressionKind::ArraySet {
                    ptr, index, value, ..
                } => {
                    self.visit_expression(ptr);
                    self.visit_expression(index);
                    self.visit_expression(value);
                }
                ExpressionKind::ArrayLen { ptr } => {
                    self.visit_expression(ptr);
                }
                ExpressionKind::Try {
                    body, catch_bodies, ..
                } => {
                    self.visit_expression(body);
                    for e in catch_bodies.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::Throw { operands, .. } => {
                    for e in operands.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::RefTest { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::RefCast { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::BrOn { value, .. } => {
                    self.visit_expression(value);
                }
                // No children for these variants
                ExpressionKind::Const(_)
                | ExpressionKind::LocalGet { .. }
                | ExpressionKind::GlobalGet { .. }
                | ExpressionKind::Unreachable
                | ExpressionKind::Nop
                | ExpressionKind::AtomicFence
                | ExpressionKind::RefNull { .. }
                | ExpressionKind::RefFunc { .. }
                | ExpressionKind::TableSize { .. }
                | ExpressionKind::MemorySize { .. }
                | ExpressionKind::DataDrop { .. }
                | ExpressionKind::ElemDrop { .. }
                | ExpressionKind::Rethrow { .. }
                | ExpressionKind::Pop { .. } => {}
            }
        }

        // After visiting children, perform the copy coalescing logic for the current expression
        if let Some(expr_mut) = unsafe { expr.as_ptr().as_mut() } {
            if let ExpressionKind::LocalSet {
                index: set_idx,
                value,
            } = &mut expr_mut.kind
            {
                if let Some(value_expr_mut) = unsafe { value.as_ptr().as_mut() } {
                    if let ExpressionKind::LocalGet { index: get_idx } = &value_expr_mut.kind {
                        // Check if the set index and get index are the same after remapping
                        // (Assuming LocalMapper has already run and updated indices)
                        if set_idx == get_idx {
                            // This is a redundant copy: local.set X (local.get X)
                            // Replace the LocalSet with a Drop of its value if it has a type, or a Nop if it's Type::NONE
                            // This effectively removes the redundant copy.
                            // The original type of the LocalSet becomes the type of the value (LocalGet's type)
                            // if it's not Type::NONE.
                            if expr_mut.type_ != Type::NONE {
                                expr_mut.kind = ExpressionKind::Drop {
                                    value: value.clone(),
                                };
                            } else {
                                expr_mut.kind = ExpressionKind::Nop;
                            }
                            // The type of the expression should be updated to Type::NONE (if it's a Nop or Drop)
                            expr_mut.type_ = Type::NONE;
                        }
                    }
                }
            }
        }
    }
}

impl CoalesceLocals {
    pub fn run_on_func(func: &mut Function) {
        let num_params = Self::count_types(func.params);

        let mut types = Vec::new();
        Self::append_types(&mut types, func.params);
        for var_type in func.vars().iter().copied() {
            Self::append_types(&mut types, var_type);
        }

        let mut mapping;
        let mut new_vars;

        if let Some(body) = func.body_mut() {
            let root = body;

            let builder = CFGBuilder::new();
            let mut cfg = builder.build(root);
            cfg.calculate_liveness();

            let interference = cfg.calculate_interference();

            mapping = Self::color(num_params as u32, &types, &interference);

            new_vars = Vec::new();
            for (old_idx, &new_idx) in mapping.iter().enumerate() {
                if new_idx >= num_params as u32 {
                    let internal_idx = (new_idx - num_params as u32) as usize;
                    if internal_idx >= new_vars.len() {
                        new_vars.resize(internal_idx + 1, Type::NONE);
                    }
                    new_vars[internal_idx] = types[old_idx];
                }
            }

            // Remap instructions
            let mut mapper = LocalMapper {
                mapping: &mut mapping,
            };
            mapper.visit(root);

            // Remove dead stores after coalescing and remapping
            Self::eliminate_dead_stores(root);
        } else {
            return;
        }

        // Update vars *after* the mapping visit to avoid borrow conflicts
        *func.vars_mut() = new_vars;

        if let Some(body) = func.body_mut() {
            let root = body;
            // Perform copy coalescing after remapping and dead store elimination
            let mut copy_coalescer = CopyCoalescingVisitor {
                _phantom: std::marker::PhantomData,
            };
            copy_coalescer.visit(root);
        }
    }

    fn count_types(ty: Type) -> usize {
        ty.tuple_len()
    }

    fn append_types(list: &mut Vec<Type>, ty: Type) {
        for element_type in ty.tuple_elements() {
            if element_type == Type::NONE {
                continue;
            }
            list.push(element_type);
        }
    }

    fn color(num_params: u32, types: &[Type], graph: &InterferenceGraph) -> Vec<u32> {
        let num_locals = types.len() as u32;
        let mut mapping: Vec<u32> = (0..num_locals).collect();
        let mut new_vars: Vec<Vec<u32>> = Vec::new();

        for i in num_params..num_locals {
            let mut found = false;
            for (j, assigned) in new_vars.iter_mut().enumerate() {
                if assigned.is_empty() {
                    continue;
                }
                let type_j = types[assigned[0] as usize];
                if types[i as usize] != type_j {
                    continue;
                }

                let mut interferes = false;
                for &other in assigned.iter() {
                    if graph.interferes(i, other) {
                        interferes = true;
                        break;
                    }
                }

                if !interferes {
                    mapping[i as usize] = num_params + j as u32;
                    assigned.push(i);
                    found = true;
                    break;
                }
            }

            if !found {
                mapping[i as usize] = num_params + new_vars.len() as u32;
                new_vars.push(vec![i]);
            }
        }

        mapping
    }

    // ---------------------------------------------------------------------
    // Dead store elimination
    // ---------------------------------------------------------------------
    fn eliminate_dead_stores(root: &mut ExprRef) {
        // First pass: collect all locals that are read (LocalGet)
        let mut used: std::collections::HashSet<u32> = std::collections::HashSet::new();
        Self::collect_used_recursive(root, &mut used);

        // Second pass: replace dead LocalSet with Drop of its value
        Self::eliminate_recursive(root, &used);
    }

    // Helper for eliminate_dead_stores to collect used locals
    fn collect_used_recursive(expr: &ExprRef, used: &mut std::collections::HashSet<u32>) {
        if let Some(expr_val) = unsafe { expr.as_ptr().as_ref() } {
            match &expr_val.kind {
                ExpressionKind::LocalGet { index } => {
                    used.insert(*index);
                }
                ExpressionKind::Block { list, .. } => {
                    for e in list {
                        Self::collect_used_recursive(e, used);
                    }
                }
                ExpressionKind::If {
                    condition,
                    if_true,
                    if_false,
                } => {
                    Self::collect_used_recursive(condition, used);
                    Self::collect_used_recursive(if_true, used);
                    if let Some(f) = if_false {
                        Self::collect_used_recursive(f, used);
                    }
                }
                ExpressionKind::Loop { body, .. } => {
                    Self::collect_used_recursive(body, used);
                }
                ExpressionKind::Unary { value, .. } => {
                    Self::collect_used_recursive(value, used);
                }
                ExpressionKind::Binary { left, right, .. } => {
                    Self::collect_used_recursive(left, used);
                    Self::collect_used_recursive(right, used);
                }
                ExpressionKind::Drop { value } => {
                    Self::collect_used_recursive(value, used);
                }
                ExpressionKind::LocalSet { index: _, value } => {
                    Self::collect_used_recursive(value, used);
                }
                ExpressionKind::LocalTee { index: _, value } => {
                    Self::collect_used_recursive(value, used);
                }
                // Call, GlobalSet, If, Loop, Drop, Select, Load, Store, Switch, CallIndirect, MemoryGrow
                // AtomicRMW, AtomicCmpxchg, AtomicWait, AtomicNotify, TupleMake, TupleExtract,
                // RefIsNull, RefEq, RefAs, TableGet, TableSet, TableGrow, TableFill, TableCopy, TableInit,
                // MemoryInit, MemoryCopy, MemoryFill, I31New, I31Get, SIMDExtract, SIMDReplace, SIMDShuffle,
                // SIMDTernary, SIMDShift, SIMDLoad, SIMDLoadStoreLane, StructNew, StructGet, StructSet, ArrayNew,
                // ArrayGet, ArraySet, ArrayLen, Try, Throw, RefTest, RefCast, BrOn, Break, Return
                //
                // Exhaustive list needed here
                ExpressionKind::Call { operands, .. } => {
                    for e in operands {
                        Self::collect_used_recursive(e, used);
                    }
                }
                ExpressionKind::GlobalSet { value, .. } => {
                    Self::collect_used_recursive(value, used);
                }
                ExpressionKind::Select {
                    condition,
                    if_true,
                    if_false,
                } => {
                    Self::collect_used_recursive(condition, used);
                    Self::collect_used_recursive(if_true, used);
                    Self::collect_used_recursive(if_false, used);
                }
                ExpressionKind::Load { ptr, .. } => {
                    Self::collect_used_recursive(ptr, used);
                }
                ExpressionKind::Store { ptr, value, .. } => {
                    Self::collect_used_recursive(ptr, used);
                    Self::collect_used_recursive(value, used);
                }
                ExpressionKind::Switch {
                    condition, value, ..
                } => {
                    Self::collect_used_recursive(condition, used);
                    if let Some(v) = value {
                        Self::collect_used_recursive(v, used);
                    }
                }
                ExpressionKind::CallIndirect {
                    target, operands, ..
                } => {
                    Self::collect_used_recursive(target, used);
                    for e in operands {
                        Self::collect_used_recursive(e, used);
                    }
                }
                ExpressionKind::MemoryGrow { delta, .. } => {
                    Self::collect_used_recursive(delta, used);
                }
                ExpressionKind::AtomicRMW { ptr, value, .. } => {
                    Self::collect_used_recursive(ptr, used);
                    Self::collect_used_recursive(value, used);
                }
                ExpressionKind::AtomicCmpxchg {
                    ptr,
                    expected,
                    replacement,
                    ..
                } => {
                    Self::collect_used_recursive(ptr, used);
                    Self::collect_used_recursive(expected, used);
                    Self::collect_used_recursive(replacement, used);
                }
                ExpressionKind::AtomicWait {
                    ptr,
                    expected,
                    timeout,
                    ..
                } => {
                    Self::collect_used_recursive(ptr, used);
                    Self::collect_used_recursive(expected, used);
                    Self::collect_used_recursive(timeout, used);
                }
                ExpressionKind::AtomicNotify { ptr, count, .. } => {
                    Self::collect_used_recursive(ptr, used);
                    Self::collect_used_recursive(count, used);
                }
                ExpressionKind::TupleMake { operands } => {
                    for e in operands {
                        Self::collect_used_recursive(e, used);
                    }
                }
                ExpressionKind::TupleExtract { tuple, .. } => {
                    Self::collect_used_recursive(tuple, used);
                }
                ExpressionKind::RefIsNull { value } => {
                    Self::collect_used_recursive(value, used);
                }
                ExpressionKind::RefEq { left, right } => {
                    Self::collect_used_recursive(left, used);
                    Self::collect_used_recursive(right, used);
                }
                ExpressionKind::RefAs { value, .. } => {
                    Self::collect_used_recursive(value, used);
                }
                ExpressionKind::TableGet { index, .. } => {
                    Self::collect_used_recursive(index, used);
                }
                ExpressionKind::TableSet { index, value, .. } => {
                    Self::collect_used_recursive(index, used);
                    Self::collect_used_recursive(value, used);
                }
                ExpressionKind::TableGrow { value, delta, .. } => {
                    Self::collect_used_recursive(value, used);
                    Self::collect_used_recursive(delta, used);
                }
                ExpressionKind::TableFill {
                    dest, value, size, ..
                } => {
                    Self::collect_used_recursive(dest, used);
                    Self::collect_used_recursive(value, used);
                    Self::collect_used_recursive(size, used);
                }
                ExpressionKind::TableCopy {
                    dest, src, size, ..
                } => {
                    Self::collect_used_recursive(dest, used);
                    Self::collect_used_recursive(src, used);
                    Self::collect_used_recursive(size, used);
                }
                ExpressionKind::TableInit {
                    dest, offset, size, ..
                } => {
                    Self::collect_used_recursive(dest, used);
                    Self::collect_used_recursive(offset, used);
                    Self::collect_used_recursive(size, used);
                }
                ExpressionKind::MemoryInit {
                    dest, offset, size, ..
                } => {
                    Self::collect_used_recursive(dest, used);
                    Self::collect_used_recursive(offset, used);
                    Self::collect_used_recursive(size, used);
                }
                ExpressionKind::MemoryCopy {
                    dest, src, size, ..
                } => {
                    Self::collect_used_recursive(dest, used);
                    Self::collect_used_recursive(src, used);
                    Self::collect_used_recursive(size, used);
                }
                ExpressionKind::MemoryFill {
                    dest, value, size, ..
                } => {
                    Self::collect_used_recursive(dest, used);
                    Self::collect_used_recursive(value, used);
                    Self::collect_used_recursive(size, used);
                }
                ExpressionKind::I31New { value } => {
                    Self::collect_used_recursive(value, used);
                }
                ExpressionKind::I31Get { i31, .. } => {
                    Self::collect_used_recursive(i31, used);
                }
                ExpressionKind::SIMDExtract { vec, .. } => {
                    Self::collect_used_recursive(vec, used);
                }
                ExpressionKind::SIMDReplace { vec, value, .. } => {
                    Self::collect_used_recursive(vec, used);
                    Self::collect_used_recursive(value, used);
                }
                ExpressionKind::SIMDShuffle { left, right, .. } => {
                    Self::collect_used_recursive(left, used);
                    Self::collect_used_recursive(right, used);
                }
                ExpressionKind::SIMDTernary { a, b, c, .. } => {
                    Self::collect_used_recursive(a, used);
                    Self::collect_used_recursive(b, used);
                    Self::collect_used_recursive(c, used);
                }
                ExpressionKind::SIMDShift { vec, shift, .. } => {
                    Self::collect_used_recursive(vec, used);
                    Self::collect_used_recursive(shift, used);
                }
                ExpressionKind::SIMDLoad { ptr, .. } => {
                    Self::collect_used_recursive(ptr, used);
                }
                ExpressionKind::SIMDLoadStoreLane { ptr, vec, .. } => {
                    Self::collect_used_recursive(ptr, used);
                    Self::collect_used_recursive(vec, used);
                }
                ExpressionKind::StructNew { operands, .. } => {
                    for e in operands {
                        Self::collect_used_recursive(e, used);
                    }
                }
                ExpressionKind::StructGet { ptr, .. } => {
                    Self::collect_used_recursive(ptr, used);
                }
                ExpressionKind::StructSet { ptr, value, .. } => {
                    Self::collect_used_recursive(ptr, used);
                    Self::collect_used_recursive(value, used);
                }
                ExpressionKind::ArrayNew { size, init, .. } => {
                    Self::collect_used_recursive(size, used);
                    if let Some(i) = init {
                        Self::collect_used_recursive(i, used);
                    }
                }
                ExpressionKind::ArrayGet { ptr, index, .. } => {
                    Self::collect_used_recursive(ptr, used);
                    Self::collect_used_recursive(index, used);
                }
                ExpressionKind::ArraySet {
                    ptr, index, value, ..
                } => {
                    Self::collect_used_recursive(ptr, used);
                    Self::collect_used_recursive(index, used);
                    Self::collect_used_recursive(value, used);
                }
                ExpressionKind::ArrayLen { ptr } => {
                    Self::collect_used_recursive(ptr, used);
                }
                ExpressionKind::Try {
                    body, catch_bodies, ..
                } => {
                    Self::collect_used_recursive(body, used);
                    for e in catch_bodies {
                        Self::collect_used_recursive(e, used);
                    }
                }
                ExpressionKind::Throw { operands, .. } => {
                    for e in operands {
                        Self::collect_used_recursive(e, used);
                    }
                }
                ExpressionKind::RefTest { value, .. } => {
                    Self::collect_used_recursive(value, used);
                }
                ExpressionKind::RefCast { value, .. } => {
                    Self::collect_used_recursive(value, used);
                }
                ExpressionKind::BrOn { value, .. } => {
                    Self::collect_used_recursive(value, used);
                }
                ExpressionKind::Break {
                    condition, value, ..
                } => {
                    if let Some(c) = condition {
                        Self::collect_used_recursive(c, used);
                    }
                    if let Some(v) = value {
                        Self::collect_used_recursive(v, used);
                    }
                }
                ExpressionKind::Return { value } => {
                    if let Some(v) = value {
                        Self::collect_used_recursive(v, used);
                    }
                }
                ExpressionKind::Const(_)
                | ExpressionKind::GlobalGet { .. }
                | ExpressionKind::Unreachable
                | ExpressionKind::Nop
                | ExpressionKind::AtomicFence
                | ExpressionKind::RefNull { .. }
                | ExpressionKind::RefFunc { .. }
                | ExpressionKind::TableSize { .. }
                | ExpressionKind::MemorySize { .. }
                | ExpressionKind::DataDrop { .. }
                | ExpressionKind::ElemDrop { .. }
                | ExpressionKind::Rethrow { .. }
                | ExpressionKind::Pop { .. } => {}
            }
        }
    }

    // Helper for eliminate_dead_stores to perform elimination
    fn eliminate_recursive(expr: &mut ExprRef, used: &std::collections::HashSet<u32>) {
        if let Some(expr_mut) = unsafe { expr.as_ptr().as_mut() } {
            match &mut expr_mut.kind {
                ExpressionKind::Block { list, .. } => {
                    for e in list.iter_mut() {
                        Self::eliminate_recursive(e, used);
                    }
                }
                ExpressionKind::If {
                    condition,
                    if_true,
                    if_false,
                } => {
                    Self::eliminate_recursive(condition, used);
                    Self::eliminate_recursive(if_true, used);
                    if let Some(f) = if_false {
                        Self::eliminate_recursive(f, used);
                    }
                }
                ExpressionKind::Loop { body, .. } => {
                    Self::eliminate_recursive(body, used);
                }
                ExpressionKind::Unary { value, .. } => {
                    Self::eliminate_recursive(value, used);
                }
                ExpressionKind::Binary { left, right, .. } => {
                    Self::eliminate_recursive(left, used);
                    Self::eliminate_recursive(right, used);
                }
                ExpressionKind::Call { operands, .. } => {
                    for e in operands.iter_mut() {
                        Self::eliminate_recursive(e, used);
                    }
                }
                ExpressionKind::LocalSet { index, value } => {
                    if !used.contains(index) {
                        expr_mut.kind = ExpressionKind::Drop {
                            value: value.clone(),
                        };
                    } else {
                        Self::eliminate_recursive(value, used);
                    }
                }
                ExpressionKind::LocalTee { index, value } => {
                    if !used.contains(index) {
                        expr_mut.kind = ExpressionKind::Drop {
                            value: value.clone(),
                        };
                    } else {
                        Self::eliminate_recursive(value, used);
                    }
                }
                ExpressionKind::GlobalSet { value, .. } => {
                    Self::eliminate_recursive(value, used);
                }
                ExpressionKind::Break {
                    condition, value, ..
                } => {
                    if let Some(c) = condition {
                        Self::eliminate_recursive(c, used);
                    }
                    if let Some(v) = value {
                        Self::eliminate_recursive(v, used);
                    }
                }
                ExpressionKind::Return { value } => {
                    if let Some(v) = value {
                        Self::eliminate_recursive(v, used);
                    }
                }
                ExpressionKind::Drop { value } => {
                    Self::eliminate_recursive(value, used);
                }
                ExpressionKind::Select {
                    condition,
                    if_true,
                    if_false,
                } => {
                    Self::eliminate_recursive(condition, used);
                    Self::eliminate_recursive(if_true, used);
                    Self::eliminate_recursive(if_false, used);
                }
                ExpressionKind::Load { ptr, .. } => {
                    Self::eliminate_recursive(ptr, used);
                }
                ExpressionKind::Store { ptr, value, .. } => {
                    Self::eliminate_recursive(ptr, used);
                    Self::eliminate_recursive(value, used);
                }
                ExpressionKind::Switch {
                    condition, value, ..
                } => {
                    Self::eliminate_recursive(condition, used);
                    if let Some(v) = value {
                        Self::eliminate_recursive(v, used);
                    }
                }
                ExpressionKind::CallIndirect {
                    target, operands, ..
                } => {
                    Self::eliminate_recursive(target, used);
                    for e in operands.iter_mut() {
                        Self::eliminate_recursive(e, used);
                    }
                }
                ExpressionKind::MemoryGrow { delta, .. } => {
                    Self::eliminate_recursive(delta, used);
                }
                ExpressionKind::AtomicRMW { ptr, value, .. } => {
                    Self::eliminate_recursive(ptr, used);
                    Self::eliminate_recursive(value, used);
                }
                ExpressionKind::AtomicCmpxchg {
                    ptr,
                    expected,
                    replacement,
                    ..
                } => {
                    Self::eliminate_recursive(ptr, used);
                    Self::eliminate_recursive(expected, used);
                    Self::eliminate_recursive(replacement, used);
                }
                ExpressionKind::AtomicWait {
                    ptr,
                    expected,
                    timeout,
                    ..
                } => {
                    Self::eliminate_recursive(ptr, used);
                    Self::eliminate_recursive(expected, used);
                    Self::eliminate_recursive(timeout, used);
                }
                ExpressionKind::AtomicNotify { ptr, count, .. } => {
                    Self::eliminate_recursive(ptr, used);
                    Self::eliminate_recursive(count, used);
                }
                ExpressionKind::TupleMake { operands } => {
                    for e in operands.iter_mut() {
                        Self::eliminate_recursive(e, used);
                    }
                }
                ExpressionKind::TupleExtract { tuple, .. } => {
                    Self::eliminate_recursive(tuple, used);
                }
                ExpressionKind::RefIsNull { value } => {
                    Self::eliminate_recursive(value, used);
                }
                ExpressionKind::RefEq { left, right } => {
                    Self::eliminate_recursive(left, used);
                    Self::eliminate_recursive(right, used);
                }
                ExpressionKind::RefAs { value, .. } => {
                    Self::eliminate_recursive(value, used);
                }
                ExpressionKind::TableGet { index, .. } => {
                    Self::eliminate_recursive(index, used);
                }
                ExpressionKind::TableSet { index, value, .. } => {
                    Self::eliminate_recursive(index, used);
                    Self::eliminate_recursive(value, used);
                }
                ExpressionKind::TableGrow { value, delta, .. } => {
                    Self::eliminate_recursive(value, used);
                    Self::eliminate_recursive(delta, used);
                }
                ExpressionKind::TableFill {
                    dest, value, size, ..
                } => {
                    Self::eliminate_recursive(dest, used);
                    Self::eliminate_recursive(value, used);
                    Self::eliminate_recursive(size, used);
                }
                ExpressionKind::TableCopy {
                    dest, src, size, ..
                } => {
                    Self::eliminate_recursive(dest, used);
                    Self::eliminate_recursive(src, used);
                    Self::eliminate_recursive(size, used);
                }
                ExpressionKind::TableInit {
                    dest, offset, size, ..
                } => {
                    Self::eliminate_recursive(dest, used);
                    Self::eliminate_recursive(offset, used);
                    Self::eliminate_recursive(size, used);
                }
                ExpressionKind::MemoryInit {
                    dest, offset, size, ..
                } => {
                    Self::eliminate_recursive(dest, used);
                    Self::eliminate_recursive(offset, used);
                    Self::eliminate_recursive(size, used);
                }
                ExpressionKind::MemoryCopy {
                    dest, src, size, ..
                } => {
                    Self::eliminate_recursive(dest, used);
                    Self::eliminate_recursive(src, used);
                    Self::eliminate_recursive(size, used);
                }
                ExpressionKind::MemoryFill {
                    dest, value, size, ..
                } => {
                    Self::eliminate_recursive(dest, used);
                    Self::eliminate_recursive(value, used);
                    Self::eliminate_recursive(size, used);
                }
                ExpressionKind::I31New { value } => {
                    Self::eliminate_recursive(value, used);
                }
                ExpressionKind::I31Get { i31, .. } => {
                    Self::eliminate_recursive(i31, used);
                }
                ExpressionKind::SIMDExtract { vec, .. } => {
                    Self::eliminate_recursive(vec, used);
                }
                ExpressionKind::SIMDReplace { vec, value, .. } => {
                    Self::eliminate_recursive(vec, used);
                    Self::eliminate_recursive(value, used);
                }
                ExpressionKind::SIMDShuffle { left, right, .. } => {
                    Self::eliminate_recursive(left, used);
                    Self::eliminate_recursive(right, used);
                }
                ExpressionKind::SIMDTernary { a, b, c, .. } => {
                    Self::eliminate_recursive(a, used);
                    Self::eliminate_recursive(b, used);
                    Self::eliminate_recursive(c, used);
                }
                ExpressionKind::SIMDShift { vec, shift, .. } => {
                    Self::eliminate_recursive(vec, used);
                    Self::eliminate_recursive(shift, used);
                }
                ExpressionKind::SIMDLoad { ptr, .. } => {
                    Self::eliminate_recursive(ptr, used);
                }
                ExpressionKind::SIMDLoadStoreLane { ptr, vec, .. } => {
                    Self::eliminate_recursive(ptr, used);
                    Self::eliminate_recursive(vec, used);
                }
                ExpressionKind::StructNew { operands, .. } => {
                    for e in operands.iter_mut() {
                        Self::eliminate_recursive(e, used);
                    }
                }
                ExpressionKind::StructGet { ptr, .. } => {
                    Self::eliminate_recursive(ptr, used);
                }
                ExpressionKind::StructSet { ptr, value, .. } => {
                    Self::eliminate_recursive(ptr, used);
                    Self::eliminate_recursive(value, used);
                }
                ExpressionKind::ArrayNew { size, init, .. } => {
                    Self::eliminate_recursive(size, used);
                    if let Some(i) = init {
                        Self::eliminate_recursive(i, used);
                    }
                }
                ExpressionKind::ArrayGet { ptr, index, .. } => {
                    Self::eliminate_recursive(ptr, used);
                    Self::eliminate_recursive(index, used);
                }
                ExpressionKind::ArraySet {
                    ptr, index, value, ..
                } => {
                    Self::eliminate_recursive(ptr, used);
                    Self::eliminate_recursive(index, used);
                    Self::eliminate_recursive(value, used);
                }
                ExpressionKind::ArrayLen { ptr } => {
                    Self::eliminate_recursive(ptr, used);
                }
                ExpressionKind::Try {
                    body, catch_bodies, ..
                } => {
                    Self::eliminate_recursive(body, used);
                    for e in catch_bodies.iter_mut() {
                        Self::eliminate_recursive(e, used);
                    }
                }
                ExpressionKind::Throw { operands, .. } => {
                    for e in operands.iter_mut() {
                        Self::eliminate_recursive(e, used);
                    }
                }
                ExpressionKind::RefTest { value, .. } => {
                    Self::eliminate_recursive(value, used);
                }
                ExpressionKind::RefCast { value, .. } => {
                    Self::eliminate_recursive(value, used);
                }
                ExpressionKind::BrOn { value, .. } => {
                    Self::eliminate_recursive(value, used);
                }
                // No children for these variants
                ExpressionKind::Const(_)
                | ExpressionKind::LocalGet { .. }
                | ExpressionKind::GlobalGet { .. }
                | ExpressionKind::Unreachable
                | ExpressionKind::Nop
                | ExpressionKind::AtomicFence
                | ExpressionKind::RefNull { .. }
                | ExpressionKind::RefFunc { .. }
                | ExpressionKind::TableSize { .. }
                | ExpressionKind::MemorySize { .. }
                | ExpressionKind::DataDrop { .. }
                | ExpressionKind::ElemDrop { .. }
                | ExpressionKind::Rethrow { .. }
                | ExpressionKind::Pop { .. } => {}
            }
        }
    }
}
