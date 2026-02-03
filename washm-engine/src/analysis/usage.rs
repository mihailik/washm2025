use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::{
    DataId, ElementId, FuncId, GlobalId, GlobalKind, MemoryId, Module, TableId, TypeId,
};
use crate::ir::visitor::ReadOnlyVisitor;
use std::collections::{HashSet, VecDeque};

/// Tracks usage of various module elements to determine reachability.
#[derive(Debug, Default)]
pub struct UsageTracker {
    pub functions: HashSet<FuncId>,
    pub globals: HashSet<GlobalId>,
    pub memories: HashSet<MemoryId>,
    pub tables: HashSet<TableId>,
    pub element_segments: HashSet<ElementId>,
    pub data_segments: HashSet<DataId>,
    pub types: HashSet<TypeId>,

    pub(crate) func_queue: VecDeque<FuncId>,
    pub(crate) global_queue: VecDeque<GlobalId>,
}

impl UsageTracker {
    pub fn analyze(module: &Module) -> Self {
        let mut tracker = Self::default();

        // 1. Initial seeds (roots)

        // Exports
        for export in &module.exports {
            match export.kind {
                crate::ir::module::ExportKind::Function(id) => {
                    tracker.mark_func(id);
                }
                crate::ir::module::ExportKind::Global(id) => {
                    tracker.mark_global(id);
                }
                crate::ir::module::ExportKind::Memory(id) => {
                    tracker.memories.insert(id);
                }
                crate::ir::module::ExportKind::Table(id) => {
                    tracker.tables.insert(id);
                }
            }
        }

        // Start function
        if let Some(start_id) = module.start {
            tracker.mark_func(start_id);
        }

        // 2. Transitive closure (worklist)
        while !tracker.func_queue.is_empty() || !tracker.global_queue.is_empty() {
            while let Some(func_id) = tracker.func_queue.pop_front() {
                if let Some(func) = module.functions.get(func_id) {
                    if let Some(type_idx) = func.type_idx {
                        tracker.types.insert(type_idx);
                    }
                    if let Some(body_expr) = func.body() {
                        let mut visitor = UsageVisitor {
                            tracker: &mut tracker,
                        };
                        visitor.visit(body_expr, &module.expressions);
                    }
                }
            }

            while let Some(global_id) = tracker.global_queue.pop_front() {
                if let Some(global) = module.globals.get(global_id) {
                    if let GlobalKind::Local { init } = &global.kind {
                        let mut visitor = UsageVisitor {
                            tracker: &mut tracker,
                        };
                        visitor.visit(*init, &module.expressions);
                    }
                }
            }
        }

        // 3. Finalize segments based on memory/table preservation
        if !tracker.memories.is_empty() {
            for (id, _) in module.data.iter() {
                tracker.data_segments.insert(id);
            }
        }
        if !tracker.tables.is_empty() {
            for (id, elem) in module.elements.iter() {
                tracker.element_segments.insert(id);
                // Functions in active segments are used.
                for &func_id in &elem.func_indices {
                    tracker.mark_func(func_id);
                }
            }
        }

        tracker
    }

    fn mark_func(&mut self, id: FuncId) {
        if self.functions.insert(id) {
            self.func_queue.push_back(id);
        }
    }

    fn mark_global(&mut self, id: GlobalId) {
        if self.globals.insert(id) {
            self.global_queue.push_back(id);
        }
    }
}

struct UsageVisitor<'a> {
    tracker: &'a mut UsageTracker,
}

impl<'a> ReadOnlyVisitor for UsageVisitor<'a> {
    fn visit_expression(
        &mut self,
        expr: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        match &arena[expr].kind {
            ExpressionKind::Call { target, .. } | ExpressionKind::RefFunc { func: target } => {
                self.tracker.mark_func(*target);
            }
            ExpressionKind::GlobalGet { id } | ExpressionKind::GlobalSet { id, .. } => {
                self.tracker.mark_global(*id);
            }
            ExpressionKind::Load { memory, .. }
            | ExpressionKind::Store { memory, .. }
            | ExpressionKind::AtomicRMW { memory, .. }
            | ExpressionKind::AtomicCmpxchg { memory, .. }
            | ExpressionKind::AtomicWait { memory, .. }
            | ExpressionKind::AtomicNotify { memory, .. }
            | ExpressionKind::MemorySize { memory, .. }
            | ExpressionKind::MemoryGrow { memory, .. }
            | ExpressionKind::MemoryFill { memory, .. } => {
                self.tracker.memories.insert(*memory);
            }
            ExpressionKind::MemoryCopy {
                dest_memory,
                src_memory,
                ..
            } => {
                self.tracker.memories.insert(*dest_memory);
                self.tracker.memories.insert(*src_memory);
            }
            ExpressionKind::MemoryInit {
                segment, memory, ..
            } => {
                self.tracker.memories.insert(*memory);
                self.tracker.data_segments.insert(*segment);
            }
            ExpressionKind::DataDrop { segment } => {
                self.tracker.data_segments.insert(*segment);
            }
            ExpressionKind::TableInit { table, segment, .. } => {
                self.tracker.tables.insert(*table);
                self.tracker.element_segments.insert(*segment);
            }
            ExpressionKind::ElemDrop { segment } => {
                self.tracker.element_segments.insert(*segment);
            }
            ExpressionKind::CallIndirect { table, type_id, .. } => {
                self.tracker.tables.insert(*table);
                self.tracker.types.insert(*type_id);
            }
            ExpressionKind::TableGet { table, .. }
            | ExpressionKind::TableSet { table, .. }
            | ExpressionKind::TableSize { table, .. }
            | ExpressionKind::TableGrow { table, .. }
            | ExpressionKind::TableFill { table, .. } => {
                self.tracker.tables.insert(*table);
            }
            ExpressionKind::TableCopy {
                dest_table,
                src_table,
                ..
            } => {
                self.tracker.tables.insert(*dest_table);
                self.tracker.tables.insert(*src_table);
            }
            _ => {}
        }
    }
}
