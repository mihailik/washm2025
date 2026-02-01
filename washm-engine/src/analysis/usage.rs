use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::{GlobalId, GlobalKind, ImportKind, Module};
use crate::ir::visitor::ReadOnlyVisitor;
use std::collections::{HashSet, VecDeque};

/// Tracks usage of various module elements to determine reachability.
#[derive(Debug, Default)]
pub struct UsageTracker {
    pub functions: HashSet<String>,
    pub globals: HashSet<GlobalId>,
    pub memories: bool,
    pub tables: bool,
    pub element_segments: HashSet<u32>,
    pub data_segments: HashSet<u32>,
    pub types: HashSet<u32>,

    pub(crate) func_queue: VecDeque<String>,
    pub(crate) global_queue: VecDeque<GlobalId>,
}

impl UsageTracker {
    pub fn analyze(module: &Module) -> Self {
        let mut tracker = Self::default();

        // 1. Initial seeds (roots)

        // Exports
        for export in &module.exports {
            match export.kind {
                crate::ir::module::ExportKind::Function(index) => {
                    if let Some(name) = module_get_func_name(module, index) {
                        tracker.mark_func(&name);
                    }
                }
                crate::ir::module::ExportKind::Global(id) => {
                    tracker.mark_global(id);
                }
                crate::ir::module::ExportKind::Memory(_) => tracker.memories = true,
                crate::ir::module::ExportKind::Table(_) => tracker.tables = true,
            }
        }

        // Start function
        if let Some(start_idx) = module.start {
            if let Some(name) = module_get_func_name(module, start_idx) {
                tracker.mark_func(&name);
            }
        }

        // 2. Transitive closure (worklist)
        while !tracker.func_queue.is_empty() || !tracker.global_queue.is_empty() {
            while let Some(func_name) = tracker.func_queue.pop_front() {
                if let Some(func) = module.get_function(&func_name) {
                    if let Some(type_idx) = func.type_idx {
                        tracker.types.insert(type_idx);
                    }
                    if let Some(body) = func.body {
                        let mut visitor = UsageVisitor {
                            tracker: &mut tracker,
                        };
                        visitor.visit(body);
                    }
                }
            }

            while let Some(global_id) = tracker.global_queue.pop_front() {
                if let Some(global) = module.globals.get(global_id.cast()) {
                    if let GlobalKind::Local { init } = global.kind {
                        let mut visitor = UsageVisitor {
                            tracker: &mut tracker,
                        };
                        visitor.visit(init);
                    }
                }
            }
        }

        // 3. Finalize segments based on memory/table preservation
        // Active segments are only kept if their target is kept.
        // Passive segments were already marked in ExposureVisitor if used.
        if tracker.memories {
            for (i, _data) in module.data.iter().enumerate() {
                // In this IR, DataSegment always has an offset. If it had no offset, it would be passive.
                // Assuming all segments in module.data are active for now (as per parser).
                tracker.data_segments.insert(i as u32);
            }
        }
        if tracker.tables {
            for (i, elem) in module.elements.iter().enumerate() {
                tracker.element_segments.insert(i as u32);
                // Functions in active segments are used.
                for &func_idx in &elem.func_indices {
                    if let Some(name) = module_get_func_name(module, func_idx) {
                        tracker.mark_func(&name);
                    }
                }
            }
        }

        tracker
    }

    fn mark_func(&mut self, name: &str) {
        if self.functions.insert(name.to_string()) {
            self.func_queue.push_back(name.to_string());
        }
    }

    fn mark_global(&mut self, id: GlobalId) {
        if self.globals.insert(id) {
            self.global_queue.push_back(id);
        }
    }
}

fn module_get_func_name(module: &Module, index: u32) -> Option<String> {
    let mut current_idx = 0;
    for import in &module.imports {
        if let ImportKind::Function(_, _) = import.kind {
            if current_idx == index {
                return Some(import.name.clone());
            }
            current_idx += 1;
        }
    }

    let defined_idx = index - current_idx;
    module
        .functions
        .get(defined_idx as usize)
        .map(|f| f.name.clone())
}

struct UsageVisitor<'a> {
    tracker: &'a mut UsageTracker,
}

impl<'a, 'b> ReadOnlyVisitor<'b> for UsageVisitor<'a> {
    fn visit_expression(&mut self, expr: ExprRef<'b>) {
        match &expr.kind {
            ExpressionKind::Call { target, .. } => {
                self.tracker.mark_func(target);
            }
            ExpressionKind::GlobalGet { id } | ExpressionKind::GlobalSet { id, .. } => {
                self.tracker.mark_global(*id);
            }
            ExpressionKind::Load { .. }
            | ExpressionKind::Store { .. }
            | ExpressionKind::AtomicRMW { .. }
            | ExpressionKind::AtomicCmpxchg { .. }
            | ExpressionKind::AtomicWait { .. }
            | ExpressionKind::AtomicNotify { .. }
            | ExpressionKind::MemoryCopy { .. }
            | ExpressionKind::MemoryFill { .. }
            | ExpressionKind::MemorySize
            | ExpressionKind::MemoryGrow { .. } => {
                self.tracker.memories = true;
            }
            ExpressionKind::MemoryInit { segment, .. } | ExpressionKind::DataDrop { segment } => {
                self.tracker.memories = true;
                self.tracker.data_segments.insert(*segment);
            }
            ExpressionKind::TableInit {
                table: _, segment, ..
            }
            | ExpressionKind::ElemDrop { segment } => {
                self.tracker.tables = true;
                self.tracker.element_segments.insert(*segment);
            }
            ExpressionKind::CallIndirect { type_, .. } => {
                self.tracker.tables = true;
                if let Some(id) = type_.signature_id() {
                    self.tracker.types.insert(id);
                }
            }
            ExpressionKind::TableGet { .. }
            | ExpressionKind::TableSet { .. }
            | ExpressionKind::TableSize { .. }
            | ExpressionKind::TableGrow { .. }
            | ExpressionKind::TableFill { .. }
            | ExpressionKind::TableCopy { .. } => {
                self.tracker.tables = true;
            }
            ExpressionKind::RefFunc { func } => {
                self.tracker.mark_func(func);
            }
            _ => {}
        }
    }
}
