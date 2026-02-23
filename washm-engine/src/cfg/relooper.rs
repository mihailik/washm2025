use crate::collections::id_arena::{Id, IdArena};
use crate::ir::expression::{
    Expression, ExpressionId, ExpressionKind, ExpressionMarker, IrBuilder,
};
use crate::ir::{Literal, LocalId, Type};
use std::collections::{HashMap, HashSet};

// --- Markers & IDs ---

pub struct RelooperBlockMarker;
pub type RelooperBlockId = Id<RelooperBlockMarker>;

pub struct RelooperBranchMarker;
pub type RelooperBranchId = Id<RelooperBranchMarker>;

pub struct ShapeMarker;
pub type ShapeId = Id<ShapeMarker>;

// --- Branch ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchType {
    Direct,
    Break,
    Continue,
}

pub struct RelooperBranch {
    pub target: RelooperBlockId,
    pub branch_type: BranchType,
    pub ancestor: Option<ShapeId>,
    pub condition: Option<ExpressionId>,
    pub code: Option<ExpressionId>,
    pub switch_values: Option<Vec<u32>>,
}

// --- Block ---

pub struct RelooperBlock {
    pub id: u32,
    pub code: ExpressionId,
    pub switch_condition: Option<ExpressionId>,
    pub branches_out: HashMap<RelooperBlockId, RelooperBranchId>,
    pub branches_in: HashSet<RelooperBlockId>,
    pub processed_branches_out: HashMap<RelooperBlockId, RelooperBranchId>,
    pub processed_branches_in: HashSet<RelooperBlockId>,
    pub is_checked_multiple_entry: bool,
    pub parent: Option<ShapeId>,
}

// --- Shape ---

#[derive(Debug, Clone)]
pub enum ShapeKind {
    Simple {
        inner: RelooperBlockId,
    },
    Multiple {
        inner_map: HashMap<u32, ShapeId>,
    },
    Loop {
        inner: ShapeId,
        entries: HashSet<RelooperBlockId>,
    },
}

pub struct Shape {
    pub id: u32,
    pub kind: ShapeKind,
    pub next: Option<ShapeId>,
}

// --- Relooper ---

pub struct Relooper {
    pub blocks: IdArena<RelooperBlockMarker, RelooperBlock>,
    pub branches: IdArena<RelooperBranchMarker, RelooperBranch>,
    pub shapes: IdArena<ShapeMarker, Shape>,
    pub root: Option<ShapeId>,
    pub block_id_counter: u32,
    pub shape_id_counter: u32,
    pub rendered_shapes: std::cell::RefCell<HashSet<ShapeId>>,
    pub rendered_blocks: std::cell::RefCell<HashSet<RelooperBlockId>>,
}

impl Relooper {
    pub fn new() -> Self {
        Self {
            blocks: IdArena::new(),
            branches: IdArena::new(),
            shapes: IdArena::new(),
            root: None,
            block_id_counter: 1,
            shape_id_counter: 0,
            rendered_shapes: std::cell::RefCell::new(HashSet::new()),
            rendered_blocks: std::cell::RefCell::new(HashSet::new()),
        }
    }

    pub fn add_block(
        &mut self,
        code: ExpressionId,
        switch_condition: Option<ExpressionId>,
    ) -> RelooperBlockId {
        let id = self.block_id_counter;
        self.block_id_counter += 1;
        self.blocks.alloc(RelooperBlock {
            id,
            code,
            switch_condition,
            branches_out: HashMap::new(),
            branches_in: HashSet::new(),
            processed_branches_out: HashMap::new(),
            processed_branches_in: HashSet::new(),
            is_checked_multiple_entry: false,
            parent: None,
        })
    }

    pub fn add_branch(
        &mut self,
        from: RelooperBlockId,
        to: RelooperBlockId,
        condition: Option<ExpressionId>,
        code: Option<ExpressionId>,
    ) -> RelooperBranchId {
        let branch_id = self.branches.alloc(RelooperBranch {
            target: to,
            branch_type: BranchType::Direct,
            ancestor: None,
            condition,
            code,
            switch_values: None,
        });

        self.blocks
            .get_mut(from)
            .unwrap()
            .branches_out
            .insert(to, branch_id);
        branch_id
    }

    pub fn add_switch_branch(
        &mut self,
        from: RelooperBlockId,
        to: RelooperBlockId,
        values: Vec<u32>,
        code: Option<ExpressionId>,
    ) -> RelooperBranchId {
        let branch_id = self.branches.alloc(RelooperBranch {
            target: to,
            branch_type: BranchType::Direct,
            ancestor: None,
            condition: None,
            code,
            switch_values: Some(values),
        });

        self.blocks
            .get_mut(from)
            .unwrap()
            .branches_out
            .insert(to, branch_id);
        branch_id
    }

    pub fn calculate(
        &mut self,
        entry: RelooperBlockId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // 1. Optimize
        self.optimize(entry, arena);

        // 2. Liveness and calculate predecessors
        let mut live = HashSet::new();
        let mut stack = vec![entry];
        while let Some(curr_id) = stack.pop() {
            if !live.insert(curr_id) {
                continue;
            }
            if let Some(block) = self.blocks.get(curr_id) {
                for &target_id in block.branches_out.keys() {
                    stack.push(target_id);
                }
            }
        }

        for &block_id in &live {
            self.blocks.get_mut(block_id).unwrap().branches_in.clear();
        }

        for &block_id in &live {
            let targets: Vec<RelooperBlockId> = self
                .blocks
                .get(block_id)
                .unwrap()
                .branches_out
                .keys()
                .cloned()
                .collect();
            for target_id in targets {
                if live.contains(&target_id) {
                    self.blocks
                        .get_mut(target_id)
                        .unwrap()
                        .branches_in
                        .insert(block_id);
                }
            }
        }

        let mut initial_entries = HashSet::new();
        initial_entries.insert(entry);
        self.root = self.process(live, initial_entries);
    }

    fn optimize(
        &mut self,
        entry: RelooperBlockId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let mut more = true;
        while more {
            more = false;
            more |= self.skip_empty_blocks(arena);
            more |= self.merge_consecutive_blocks(entry, arena);
        }
    }

    fn is_block_empty(
        &self,
        block_id: RelooperBlockId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> bool {
        let block = self.blocks.get(block_id).unwrap();
        if block.switch_condition.is_some() {
            return false;
        }

        let expr = arena.get(block.code).unwrap();
        match &expr.kind {
            ExpressionKind::Block { list, .. } => list.is_empty(),
            ExpressionKind::Nop => true,
            _ => false,
        }
    }

    fn skip_empty_blocks(&mut self, _arena: &IdArena<ExpressionMarker, Expression>) -> bool {
        false
    }

    fn merge_consecutive_blocks(
        &mut self,
        entry: RelooperBlockId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) -> bool {
        let mut worked = false;

        let mut num_predecessors: HashMap<RelooperBlockId, usize> = HashMap::new();
        for (_, block) in self.blocks.iter() {
            for &target_id in block.branches_out.keys() {
                *num_predecessors.entry(target_id).or_default() += 1;
            }
        }
        *num_predecessors.entry(entry).or_default() += 1;

        let block_ids: Vec<_> = self.blocks.iter().map(|(id, _)| id).collect();
        for source_id in block_ids {
            let (target_id, branch_id) = {
                let block = self.blocks.get(source_id).unwrap();
                if block.branches_out.len() != 1 {
                    continue;
                }
                let (&t, &b) = block.branches_out.iter().next().unwrap();
                (t, b)
            };

            if target_id == source_id {
                continue;
            }

            let (num_out, has_switch) = {
                let block = self.blocks.get(source_id).unwrap();
                (block.branches_out.len(), block.switch_condition.is_some())
            };

            if num_out == 1
                && !has_switch
                && num_predecessors.get(&target_id).copied().unwrap_or(0) == 1
            {
                let branch = self.branches.get(branch_id).unwrap();
                if branch.condition.is_some() {
                    continue;
                }

                worked = true;
                let target_code = self.blocks.get(target_id).unwrap().code;
                let target_branches = self.blocks.get(target_id).unwrap().branches_out.clone();
                let target_switch = self.blocks.get(target_id).unwrap().switch_condition;
                let branch_code = branch.code;

                let mut list = Vec::new();
                let source_block = self.blocks.get_mut(source_id).unwrap();
                let source_expr_id = source_block.code;

                fn get_expr_list(
                    arena: &IdArena<ExpressionMarker, Expression>,
                    id: ExpressionId,
                ) -> Vec<ExpressionId> {
                    let expr = arena.get(id).unwrap();
                    if let ExpressionKind::Block { list, .. } = &expr.kind {
                        list.clone()
                    } else if matches!(expr.kind, ExpressionKind::Nop) {
                        Vec::new()
                    } else {
                        vec![id]
                    }
                }

                list.extend(get_expr_list(arena, source_expr_id));
                if let Some(bc) = branch_code {
                    list.extend(get_expr_list(arena, bc));
                }
                list.extend(get_expr_list(arena, target_code));

                // Try to preserve any existing block name from constituent parts.
                let mut chosen_name: Option<String> = None;
                // Search recursively within the candidate expressions for the first block name.
                let mut search_stack = list.clone();
                while let Some(id) = search_stack.pop() {
                    let expr = &arena[id];
                    if let ExpressionKind::Block {
                        name: Some(ref n), ..
                    } = &expr.kind
                    {
                        chosen_name = Some(n.clone());
                        break;
                    }
                    expr.kind.for_each_child(|c| search_stack.push(c));
                }
                let new_code = Expression::alloc(
                    arena,
                    ExpressionKind::Block {
                        name: chosen_name,
                        list,
                    },
                    Type::NONE,
                );
                source_block.code = new_code;
                source_block.branches_out = target_branches;
                source_block.switch_condition = target_switch;

                num_predecessors.insert(target_id, 0);
            }
        }

        worked
    }

    fn solipsize(
        &mut self,
        target_id: RelooperBlockId,
        branch_type: BranchType,
        ancestor: ShapeId,
        from: &HashSet<RelooperBlockId>,
    ) {
        let mut to_move = Vec::new();
        {
            let target = self.blocks.get(target_id).unwrap();
            for &prior_id in &target.branches_in {
                if from.contains(&prior_id) {
                    to_move.push(prior_id);
                }
            }
        }

        for prior_id in to_move {
            let branch_id = {
                let prior = self.blocks.get_mut(prior_id).unwrap();
                let bid = prior.branches_out.remove(&target_id).unwrap();
                prior.processed_branches_out.insert(target_id, bid);
                bid
            };

            let target = self.blocks.get_mut(target_id).unwrap();
            target.branches_in.remove(&prior_id);
            target.processed_branches_in.insert(prior_id);

            let branch = self.branches.get_mut(branch_id).unwrap();
            branch.ancestor = Some(ancestor);
            branch.branch_type = branch_type;
        }
    }

    fn make_simple(
        &mut self,
        blocks: &mut HashSet<RelooperBlockId>,
        inner_id: RelooperBlockId,
        next_entries: &mut HashSet<RelooperBlockId>,
    ) -> ShapeId {
        let shape_id = self.shapes.alloc(Shape {
            id: self.shape_id_counter,
            kind: ShapeKind::Simple { inner: inner_id },
            next: None,
        });
        self.shape_id_counter += 1;

        self.blocks.get_mut(inner_id).unwrap().parent = Some(shape_id);

        if blocks.len() > 1 {
            blocks.remove(&inner_id);
            // GetBlocksOut(inner, next_entries, &blocks)
            let (targets, branches_out) = {
                let block = self.blocks.get(inner_id).unwrap();
                (
                    block.branches_out.keys().cloned().collect::<Vec<_>>(),
                    block.branches_out.clone(),
                )
            };

            for target_id in targets {
                if blocks.contains(&target_id) {
                    next_entries.insert(target_id);
                    // Mark as direct branch for rendering to allow fallthrough or fusing
                    let branch_id = branches_out[&target_id];
                    self.blocks
                        .get_mut(inner_id)
                        .unwrap()
                        .processed_branches_out
                        .insert(target_id, branch_id);
                }
            }
        }

        shape_id
    }

    fn make_loop(
        &mut self,
        blocks: &mut HashSet<RelooperBlockId>,
        entries: HashSet<RelooperBlockId>,
        next_entries: &mut HashSet<RelooperBlockId>,
    ) -> ShapeId {
        let mut potential_reach = HashSet::new();
        let mut f_queue: Vec<_> = entries.iter().cloned().collect();

        while let Some(curr_id) = f_queue.pop() {
            if !potential_reach.contains(&curr_id) {
                potential_reach.insert(curr_id);
                let block = self.blocks.get(curr_id).unwrap();
                for &target_id in block.branches_out.keys() {
                    if blocks.contains(&target_id) {
                        f_queue.push(target_id);
                    }
                }
            }
        }

        let mut inner_blocks = HashSet::new();
        let mut b_queue: Vec<_> = entries.iter().cloned().collect();

        while let Some(curr_id) = b_queue.pop() {
            if !inner_blocks.contains(&curr_id) {
                inner_blocks.insert(curr_id);
                let block = self.blocks.get(curr_id).unwrap();
                for &prev_id in &block.branches_in {
                    if potential_reach.contains(&prev_id) {
                        b_queue.push(prev_id);
                    }
                }
            }
        }

        for &id in &inner_blocks {
            blocks.remove(&id);
        }

        for &curr_id in &inner_blocks {
            let block = self.blocks.get(curr_id).unwrap();
            for &target_id in block.branches_out.keys() {
                if !inner_blocks.contains(&target_id) {
                    next_entries.insert(target_id);
                }
            }
        }

        let placeholder_inner = self.shapes.alloc(Shape {
            id: self.shape_id_counter,
            kind: ShapeKind::Simple {
                inner: *entries.iter().next().unwrap(),
            }, // Dummy, will be replaced
            next: None,
        });
        self.shape_id_counter += 1;

        let shape_id = self.shapes.alloc(Shape {
            id: self.shape_id_counter,
            kind: ShapeKind::Loop {
                inner: placeholder_inner,
                entries: entries.clone(),
            },
            next: None,
        });
        self.shape_id_counter += 1;

        for entry_id in &entries {
            self.solipsize(*entry_id, BranchType::Continue, shape_id, &inner_blocks);
        }
        let mut next_entries_vec: Vec<_> = next_entries.iter().cloned().collect();
        // Sort to ensure deterministic output
        next_entries_vec.sort();
        for &target_id in &next_entries_vec {
            self.solipsize(target_id, BranchType::Break, shape_id, &inner_blocks);
        }

        let inner_shape = self.process(inner_blocks, entries.clone());
        self.shapes.get_mut(shape_id).unwrap().kind = ShapeKind::Loop {
            inner: inner_shape.unwrap(),
            entries,
        };

        shape_id
    }

    fn find_independent_groups(
        &self,
        blocks: &HashSet<RelooperBlockId>,
        entries: &HashSet<RelooperBlockId>,
        independent_groups: &mut HashMap<RelooperBlockId, HashSet<RelooperBlockId>>,
    ) {
        let mut ownership: HashMap<RelooperBlockId, Option<RelooperBlockId>> = HashMap::new();
        let mut queue = std::collections::VecDeque::new();

        for &entry_id in entries {
            ownership.insert(entry_id, Some(entry_id));
            independent_groups
                .entry(entry_id)
                .or_insert_with(|| HashSet::new())
                .insert(entry_id);
            queue.push_back(entry_id);
        }

        while let Some(curr_id) = queue.pop_front() {
            let owner = match ownership.get(&curr_id) {
                Some(Some(o)) => *o,
                _ => continue,
            };

            let block = self.blocks.get(curr_id).unwrap();
            for &target_id in block.branches_out.keys() {
                if !blocks.contains(&target_id) {
                    continue;
                }
                match ownership.get(&target_id) {
                    None => {
                        ownership.insert(target_id, Some(owner));
                        independent_groups
                            .get_mut(&owner)
                            .unwrap()
                            .insert(target_id);
                        queue.push_back(target_id);
                    }
                    Some(Some(target_owner)) => {
                        if *target_owner != owner {
                            // Invalidate recursion
                            self.invalidate_with_children(
                                target_id,
                                &mut ownership,
                                independent_groups,
                            );
                        }
                    }
                    Some(None) => {}
                }
            }
        }

        // Second pass for parents with different owners
        for &entry_id in entries {
            let mut to_invalidate = Vec::new();
            if let Some(group) = independent_groups.get(&entry_id) {
                for &child_id in group {
                    let child_block = self.blocks.get(child_id).unwrap();
                    for &parent_id in &child_block.branches_in {
                        if !blocks.contains(&parent_id) {
                            continue;
                        }
                        if ownership.get(&parent_id).copied().flatten() != Some(entry_id) {
                            to_invalidate.push(child_id);
                            break;
                        }
                    }
                }
            }
            for child_id in to_invalidate {
                self.invalidate_with_children(child_id, &mut ownership, independent_groups);
            }
        }

        independent_groups.retain(|_, group| !group.is_empty());
    }

    fn invalidate_with_children(
        &self,
        root_id: RelooperBlockId,
        ownership: &mut HashMap<RelooperBlockId, Option<RelooperBlockId>>,
        independent_groups: &mut HashMap<RelooperBlockId, HashSet<RelooperBlockId>>,
    ) {
        let mut stack = vec![root_id];
        while let Some(id) = stack.pop() {
            if let Some(Some(owner)) = ownership.get(&id) {
                let owner = *owner;
                if let Some(group) = independent_groups.get_mut(&owner) {
                    group.remove(&id);
                }
                ownership.insert(id, None);
                let block = self.blocks.get(id).unwrap();
                for &target_id in block.branches_out.keys() {
                    stack.push(target_id);
                }
            }
        }
    }

    fn make_multiple(
        &mut self,
        blocks: &mut HashSet<RelooperBlockId>,
        entries: &HashSet<RelooperBlockId>,
        independent_groups: HashMap<RelooperBlockId, HashSet<RelooperBlockId>>,
        next_entries: &mut HashSet<RelooperBlockId>,
        is_checked_multiple: bool,
    ) -> ShapeId {
        let independent_groups_keys: HashSet<_> = independent_groups.keys().cloned().collect();
        let shape_id = self.shapes.alloc(Shape {
            id: self.shape_id_counter,
            kind: ShapeKind::Multiple {
                inner_map: HashMap::new(),
            },
            next: None,
        });
        self.shape_id_counter += 1;

        let mut inner_map = HashMap::new();

        for (entry_id, group_blocks) in independent_groups {
            let entry_num_id = self.blocks.get(entry_id).unwrap().id;

            for &block_id in &group_blocks {
                blocks.remove(&block_id);
            }

            // Find next entries and solipsize
            let mut group_next_entries = HashSet::new();
            for &block_id in &group_blocks {
                let targets: Vec<_> = self
                    .blocks
                    .get(block_id)
                    .unwrap()
                    .branches_out
                    .keys()
                    .cloned()
                    .collect();
                for target_id in targets {
                    if !group_blocks.contains(&target_id) {
                        next_entries.insert(target_id);
                        group_next_entries.insert(target_id);
                    }
                }
            }

            for &target_id in &group_next_entries {
                self.solipsize(target_id, BranchType::Direct, shape_id, &group_blocks);
            }

            let mut group_entries = HashSet::new();
            group_entries.insert(entry_id);
            let inner_shape = self.process(group_blocks, group_entries).unwrap();
            inner_map.insert(entry_num_id, inner_shape);

            if is_checked_multiple {
                self.blocks
                    .get_mut(entry_id)
                    .unwrap()
                    .is_checked_multiple_entry = true;
            }
        }

        self.shapes.get_mut(shape_id).unwrap().kind = ShapeKind::Multiple { inner_map };

        for &entry_id in entries {
            if !independent_groups_keys.contains(&entry_id) {
                next_entries.insert(entry_id);
            }
        }

        shape_id
    }

    fn process(
        &mut self,
        mut blocks: HashSet<RelooperBlockId>,
        mut entries: HashSet<RelooperBlockId>,
    ) -> Option<ShapeId> {
        let mut ret: Option<ShapeId> = None;
        let mut prev: Option<ShapeId> = None;

        while !entries.is_empty() {
            let mut next_entries = HashSet::new();
            let current_shape_id: Option<ShapeId>;

            if entries.len() == 1 {
                let curr_id = *entries.iter().next().unwrap();
                let block = self.blocks.get(curr_id).unwrap();
                let has_internal_predecessors =
                    block.branches_in.iter().any(|&p| blocks.contains(&p));
                if !has_internal_predecessors {
                    current_shape_id =
                        Some(self.make_simple(&mut blocks, curr_id, &mut next_entries));
                } else {
                    current_shape_id =
                        Some(self.make_loop(&mut blocks, entries.clone(), &mut next_entries));
                }
            } else {
                let mut independent_groups = HashMap::new();
                self.find_independent_groups(&blocks, &entries, &mut independent_groups);

                if !independent_groups.is_empty() {
                    // Check if entries can be reached by other groups (Multiple)
                    let mut validated_groups = HashMap::new();
                    for (&entry_id, group) in &independent_groups {
                        let mut valid = true;
                        let block = self.blocks.get(entry_id).unwrap();
                        for &prior_id in &block.branches_in {
                            // Only consider predecessors within the current working set
                            if blocks.contains(&prior_id) && !group.contains(&prior_id) {
                                // Reached from another group or elsewhere in the same set
                                valid = false;
                                break;
                            }
                        }
                        if valid {
                            validated_groups.insert(entry_id, group.clone());
                        }
                    }

                    if !validated_groups.is_empty() {
                        current_shape_id = Some(self.make_multiple(
                            &mut blocks,
                            &entries,
                            validated_groups,
                            &mut next_entries,
                            true,
                        ));
                        // Add deferred entries to next_entries
                    } else {
                        // Multiple entries but no independent groups -> Loop
                        current_shape_id =
                            Some(self.make_loop(&mut blocks, entries.clone(), &mut next_entries));
                    }
                } else {
                    current_shape_id =
                        Some(self.make_loop(&mut blocks, entries.clone(), &mut next_entries));
                }
            }

            if let Some(shape_id) = current_shape_id {
                if let Some(p) = prev {
                    self.shapes.get_mut(p).unwrap().next = Some(shape_id);
                } else {
                    ret = Some(shape_id);
                }
                prev = Some(shape_id);

                if next_entries.is_empty() {
                    break;
                }
                entries = next_entries;
            } else {
                break;
            }
        }

        ret
    }

    pub fn render(&mut self, builder: &mut IrBuilder, label_var: LocalId) -> ExpressionId {
        self.rendered_shapes.borrow_mut().clear();
        self.rendered_blocks.borrow_mut().clear();
        let mut block_map = std::collections::HashMap::new();
        let mut shape_map = std::collections::HashMap::new();

        for (_, block) in &self.blocks {
            let expr = builder.block(
                None as Option<&str>,
                Vec::new(),
                crate::ir::BlockType::Empty,
            );
            block_map.insert(block.id, expr);
        }
        for (_, shape) in &self.shapes {
            let expr = builder.block(
                None as Option<&str>,
                Vec::new(),
                crate::ir::BlockType::Empty,
            );
            shape_map.insert(shape.id, expr);
        }

        if let Some(root_id) = self.root {
            let ret = self.render_shape(
                builder, label_var, root_id, false, &block_map, &shape_map, None,
            );
            ret
        } else {
            builder.nop()
        }
    }

    fn render_shape(
        &mut self,
        builder: &mut IrBuilder,
        label_var: LocalId,
        shape_id: ShapeId,
        in_loop: bool,
        block_map: &std::collections::HashMap<u32, ExpressionId>,
        shape_map: &std::collections::HashMap<u32, ExpressionId>,
        stop_at: Option<ShapeId>,
    ) -> ExpressionId {
        if Some(shape_id) == stop_at {
            return builder.nop();
        }
        // Avoid re-rendering shapes we've already done
        if let Some(shape) = self.shapes.get(shape_id) {
            if let ShapeKind::Simple { inner } = &shape.kind {
                if self.rendered_blocks.borrow().contains(inner) {
                    return block_map[&self.blocks[*inner].id];
                }
            } else {
                if self.rendered_shapes.borrow().contains(&shape_id) {
                    return shape_map[&shape.id];
                }
            }
        }
        self.rendered_shapes.borrow_mut().insert(shape_id);

        let shape_kind = self.shapes.get(shape_id).unwrap().kind.clone();
        let mut ret = match shape_kind {
            ShapeKind::Simple { inner } => {
                let inner_id = inner;
                self.rendered_blocks.borrow_mut().insert(inner_id);
                let block_id_val = self.blocks.get(inner_id).unwrap().id;
                let vocab_id = block_map[&block_id_val];
                let rendered = self.render_block(
                    builder, label_var, inner_id, shape_id, in_loop, block_map, shape_map, stop_at,
                );

                // Update the pre-allocated ID with the rendered content.
                // We must preserve any name (label) that was attached (e.g., as a break target).
                let old_name =
                    if let ExpressionKind::Block { name, .. } = &builder.arena[vocab_id].kind {
                        name.clone()
                    } else {
                        None
                    };

                let is_placeholder =
                    if let ExpressionKind::Block { list, .. } = &builder.arena[vocab_id].kind {
                        list.is_empty()
                    } else {
                        false
                    };

                if is_placeholder {
                    builder.arena[vocab_id] = builder.arena[rendered].clone();
                } else {
                    let mut new_list = vec![rendered];
                    if let ExpressionKind::Block { list, .. } = &builder.arena[vocab_id].kind {
                        for &e in list {
                            if e != rendered {
                                new_list.push(e);
                            }
                        }
                    } else {
                        new_list.push(vocab_id);
                    }
                    builder.arena[vocab_id].kind = ExpressionKind::Block {
                        name: old_name.clone(),
                        list: new_list,
                    };
                }

                if let Some(n) = old_name {
                    match &mut builder.arena[vocab_id].kind {
                        ExpressionKind::Block { name, .. }
                        | ExpressionKind::If { name, .. }
                        | ExpressionKind::Loop { name, .. } => {
                            if name.is_none() {
                                *name = Some(n);
                            }
                        }
                        _ => {}
                    }
                }
                vocab_id
            }
            ShapeKind::Multiple { inner_map } => {
                let mut sorted_keys: Vec<_> = inner_map.keys().cloned().collect();
                sorted_keys.sort();
                let shape_id_val = self.shapes.get(shape_id).unwrap().id;
                let vocab_id = shape_map[&shape_id_val];
                let inner_map_clone = inner_map.clone();
                let rendered = self.render_multiple_recursive(
                    builder,
                    label_var,
                    &sorted_keys,
                    &inner_map_clone,
                    0,
                    in_loop,
                    block_map,
                    shape_map,
                    stop_at,
                );

                let old_name =
                    if let ExpressionKind::Block { name, .. } = &builder.arena[vocab_id].kind {
                        name.clone()
                    } else {
                        None
                    };

                let is_placeholder =
                    if let ExpressionKind::Block { list, .. } = &builder.arena[vocab_id].kind {
                        list.is_empty()
                    } else {
                        false
                    };

                if is_placeholder {
                    builder.arena[vocab_id] = builder.arena[rendered].clone();
                } else {
                    let mut new_list = vec![rendered];
                    if let ExpressionKind::Block { list, .. } = &builder.arena[vocab_id].kind {
                        for &e in list {
                            if e != rendered {
                                new_list.push(e);
                            }
                        }
                    } else {
                        new_list.push(vocab_id);
                    }
                    builder.arena[vocab_id].kind = ExpressionKind::Block {
                        name: old_name.clone(),
                        list: new_list,
                    };
                }

                if let Some(n) = old_name {
                    match &mut builder.arena[vocab_id].kind {
                        ExpressionKind::Block { name, .. }
                        | ExpressionKind::If { name, .. }
                        | ExpressionKind::Loop { name, .. } => {
                            if name.is_none() {
                                *name = Some(n);
                            }
                        }
                        _ => {}
                    }
                }
                vocab_id
            }
            ShapeKind::Loop { inner, .. } => {
                let inner_id = inner;
                let body = self.render_shape(
                    builder, label_var, inner_id, true, block_map, shape_map, None,
                );
                let shape_id_val = self.shapes.get(shape_id).unwrap().id;
                let vocab_id = shape_map[&shape_id_val];

                let old_name =
                    if let ExpressionKind::Block { name, .. } = &builder.arena[vocab_id].kind {
                        name.clone()
                    } else {
                        None
                    };

                let is_placeholder =
                    if let ExpressionKind::Block { list, .. } = &builder.arena[vocab_id].kind {
                        list.is_empty()
                    } else {
                        false
                    };

                if is_placeholder {
                    builder.arena[vocab_id] = Expression {
                        kind: ExpressionKind::Loop {
                            name: Some(format!("shape$continue${}", shape_id_val)),
                            body,
                        },
                        type_: Type::from(builder.arena[body].type_),
                    };
                } else {
                    let loop_expr = builder.arena.alloc(Expression {
                        kind: ExpressionKind::Loop {
                            name: Some(format!("shape$continue${}", shape_id_val)),
                            body,
                        },
                        type_: Type::from(builder.arena[body].type_),
                    });

                    let mut new_list = vec![loop_expr];
                    if let ExpressionKind::Block { list, .. } = &builder.arena[vocab_id].kind {
                        for &e in list {
                            if e != loop_expr {
                                new_list.push(e);
                            }
                        }
                    } else {
                        new_list.push(vocab_id);
                    }
                    builder.arena[vocab_id].kind = ExpressionKind::Block {
                        name: old_name.clone(),
                        list: new_list,
                    };
                }

                if let Some(n) = old_name {
                    if let ExpressionKind::Loop { name, .. } = &mut builder.arena[vocab_id].kind {
                        if name.is_none() {
                            *name = Some(n);
                        }
                    }
                }
                vocab_id
            }
        };

        ret = self.handle_followup_multiples(
            builder, label_var, ret, shape_id, in_loop, block_map, shape_map, stop_at,
        );

        let next_id = self.shapes.get(shape_id).unwrap().next;
        if let Some(nid) = next_id {
            if Some(nid) == stop_at {
                return ret;
            }
            let next_expr = self.render_shape(
                builder, label_var, nid, in_loop, block_map, shape_map, stop_at,
            );
            // If the next expression is already our current result (due to map reuse)
            // or is contained within it, do not append it to avoid duplication.
            if next_expr == ret || self.expr_contains(builder, ret, next_expr) {
                return ret;
            }
            if self.expr_contains(builder, next_expr, ret) {
                return next_expr;
            }

            let next_type = builder.arena[next_expr].type_;
            builder.block(
                None as Option<&str>,
                vec![ret, next_expr],
                crate::ir::BlockType::from(next_type),
            )
        } else {
            ret
        }
    }

    fn render_multiple_recursive(
        &mut self,
        builder: &mut IrBuilder,
        label_var: LocalId,
        keys: &[u32],
        inner_map: &HashMap<u32, ShapeId>,
        index: usize,
        in_loop: bool,
        block_map: &std::collections::HashMap<u32, ExpressionId>,
        shape_map: &std::collections::HashMap<u32, ExpressionId>,
        stop_at: Option<ShapeId>,
    ) -> ExpressionId {
        if index >= keys.len() {
            return builder.nop();
        }

        let id = keys[index];
        let body_shape_id = inner_map[&id];
        let condition = self.make_check_label(builder, label_var, id);
        let if_true = self.render_shape(
            builder,
            label_var,
            body_shape_id,
            in_loop,
            block_map,
            shape_map,
            stop_at,
        );
        let if_false = if index + 1 < keys.len() {
            Some(self.render_multiple_recursive(
                builder,
                label_var,
                keys,
                inner_map,
                index + 1,
                in_loop,
                block_map,
                shape_map,
                stop_at,
            ))
        } else {
            None
        };

        builder.if_(
            None as Option<&str>,
            condition,
            if_true,
            if_false,
            crate::ir::BlockType::Empty,
        )
    }

    fn render_block(
        &mut self,
        builder: &mut IrBuilder,
        label_var: LocalId,
        block_id: RelooperBlockId,
        parent_shape_id: ShapeId,
        in_loop: bool,
        block_map: &std::collections::HashMap<u32, ExpressionId>,
        shape_map: &std::collections::HashMap<u32, ExpressionId>,
        stop_at: Option<ShapeId>,
    ) -> ExpressionId {
        let (
            block_code,
            is_checked_multiple_entry,
            switch_condition,
            processed_branches_out,
            _block_numeric_id,
        ) = {
            let block = self.blocks.get(block_id).unwrap();
            let block_numeric_id = block.id;
            (
                block.code,
                block.is_checked_multiple_entry,
                block.switch_condition,
                block.processed_branches_out.clone(),
                block_numeric_id,
            )
        };
        let mut list = Vec::new();

        if is_checked_multiple_entry && in_loop {
            list.push(self.make_set_label(builder, label_var, 0));
        }

        let block_expr = builder.arena.get(block_code).unwrap();
        if let ExpressionKind::Block {
            list: inner_list, ..
        } = &block_expr.kind
        {
            list.extend(inner_list.iter().cloned());
        } else {
            list.push(block_code);
        }

        if processed_branches_out.is_empty() {
            let ty = list
                .last()
                .map(|&id| builder.arena[id].type_)
                .unwrap_or(Type::NONE);
            return builder.block(None as Option<&str>, list, crate::ir::BlockType::from(ty));
        }

        let mut set_label = true;

        // Fusing
        let mut fused_info: Option<(ShapeId, HashMap<u32, ShapeId>)> = None;
        let next_id = self.shapes.get(parent_shape_id).unwrap().next;
        if let Some(nid) = next_id {
            let (next_kind, next_next) = {
                let next_shape = self.shapes.get(nid).unwrap();
                (next_shape.kind.clone(), next_shape.next)
            };
            if let ShapeKind::Multiple { inner_map } = next_kind {
                fused_info = Some((nid, inner_map.clone()));

                // Consume the multiple
                self.shapes.get_mut(parent_shape_id).unwrap().next = next_next;

                if set_label
                    && inner_map.len() == processed_branches_out.len()
                    && switch_condition.is_none()
                {
                    set_label = false;
                }
            }
        }

        if let Some(switch_cond) = switch_condition {
            let rendered = self.render_switch(
                builder,
                label_var,
                block_id,
                switch_cond,
                list,
                set_label,
                &fused_info,
                in_loop,
                block_map,
                shape_map,
                stop_at,
            );
            return rendered;
        }

        let mut default_target_info = None;
        for (&target_id, &branch_id) in &processed_branches_out {
            let branch = self.branches.get(branch_id).unwrap();
            if branch.condition.is_none() {
                default_target_info = Some((target_id, branch_id));
                break;
            }
        }

        let mut iter_targets: Vec<_> = processed_branches_out.keys().cloned().collect();
        iter_targets.sort();

        let mut root = None;
        let mut curr_if: Option<ExpressionId> = None;
        let mut remaining_conditions: Option<ExpressionId> = None;

        // Ensure default is last
        if let Some((dt_id, _)) = default_target_info {
            iter_targets.retain(|&t| t != dt_id);
            iter_targets.push(dt_id);
        }

        for (i, &target_id) in iter_targets.iter().enumerate() {
            let is_default = default_target_info.is_some() && (i == iter_targets.len() - 1);
            let branch_id = processed_branches_out[&target_id];

            let target_block_id_val = self.blocks.get(target_id).unwrap().id;
            let target_is_checked = self
                .blocks
                .get(target_id)
                .unwrap()
                .is_checked_multiple_entry;

            let set_curr_label = set_label && target_is_checked;

            let has_fused_content = fused_info
                .as_ref()
                .map(|(_, m)| m.contains_key(&target_block_id_val))
                .unwrap_or(false);

            if has_fused_content {
                let branch_mut = self.branches.get_mut(branch_id).unwrap();
                branch_mut.branch_type = BranchType::Direct;
            }

            let mut curr_content = None;
            let (target_branch_type, target_branch_condition, target_branch_code) = {
                let branch = self.branches.get(branch_id).unwrap();
                (branch.branch_type, branch.condition, branch.code)
            };

            if set_curr_label
                || target_branch_type != BranchType::Direct
                || target_branch_condition.is_some()
                || has_fused_content
                || target_branch_code.is_some()
            {
                let is_in_if_chain = !is_default || remaining_conditions.is_some();
                let mut content = self.render_branch(
                    builder,
                    label_var,
                    branch_id,
                    target_id,
                    block_map,
                    shape_map,
                    set_curr_label,
                    is_in_if_chain,
                );
                if has_fused_content {
                    let (fused_m_id, fused_map) = fused_info.as_ref().unwrap();
                    let fused_shape_id = fused_map[&target_block_id_val];
                    let fused_next_stop = self.shapes.get(*fused_m_id).unwrap().next;
                    // Branches should stop at the multiple's join point or our outer boundary.
                    let target_stop = fused_next_stop.or(stop_at);
                    let fused_rendered = self.render_shape(
                        builder,
                        label_var,
                        fused_shape_id,
                        in_loop,
                        block_map,
                        shape_map,
                        target_stop,
                    );
                    content = builder.block(
                        None as Option<&str>,
                        vec![content, fused_rendered],
                        crate::ir::BlockType::Empty,
                    );
                }
                curr_content = Some(content);
            }

            if let Some(content) = curr_content {
                if is_default {
                    let now = if let Some(rem) = remaining_conditions {
                        builder.if_(
                            None as Option<&str>,
                            rem,
                            content,
                            None,
                            crate::ir::BlockType::Empty,
                        )
                    } else {
                        content
                    };

                    if let Some(c_if) = curr_if {
                        if let ExpressionKind::If { if_false, .. } = &mut builder.arena[c_if].kind {
                            *if_false = Some(now);
                        }
                    } else {
                        root = Some(now);
                    }
                } else {
                    let cond = target_branch_condition.unwrap();
                    let now = builder.if_(
                        None as Option<&str>,
                        cond,
                        content,
                        None,
                        crate::ir::BlockType::Empty,
                    );
                    if let Some(c_if) = curr_if {
                        if let ExpressionKind::If { if_false, .. } = &mut builder.arena[c_if].kind {
                            *if_false = Some(now);
                        }
                        curr_if = Some(now);
                    } else {
                        root = Some(now);
                        curr_if = Some(now);
                    }
                }
            } else if !is_default {
                let cond = target_branch_condition.unwrap();
                let now = builder.unary(crate::ir::ops::UnaryOp::EqZInt32, cond, Type::I32);
                remaining_conditions = Some(if let Some(rem) = remaining_conditions {
                    builder.binary(crate::ir::ops::BinaryOp::AndInt32, rem, now, Type::I32)
                } else {
                    now
                });
            }

            if is_default {
                break;
            }
        }

        if let Some(r) = root {
            list.push(r);
        } else if iter_targets.is_empty() {
            // If there were no branches at all, this is a terminal block reaching unreachable
        } else {
            // No root but has targets
        }

        builder.block(None as Option<&str>, list, crate::ir::BlockType::Empty)
    }

    fn render_switch(
        &mut self,
        builder: &mut IrBuilder,
        label_var: LocalId,
        block_id: RelooperBlockId,
        condition: ExpressionId,
        mut list: Vec<ExpressionId>,
        set_label: bool,
        fused_info: &Option<(ShapeId, HashMap<u32, ShapeId>)>,
        in_loop: bool,
        block_map: &std::collections::HashMap<u32, ExpressionId>,
        shape_map: &std::collections::HashMap<u32, ExpressionId>,
        stop_at: Option<ShapeId>,
    ) -> ExpressionId {
        let (processed_branches, block_id_val) = {
            let block = self.blocks.get(block_id).unwrap();
            (block.processed_branches_out.clone(), block.id)
        };
        let base = format!("switch${}", block_id_val);
        let leave_name = format!("{}$leave", base);

        // In Binaryen, the switch is rendered by nesting blocks.
        // We'll mimic this.

        let leave_id = builder.arena.alloc(Expression {
            kind: ExpressionKind::Nop, // placeholder name will be set later
            type_: Type::NONE,
        });

        let mut current_outer = builder.arena.alloc(Expression {
            kind: ExpressionKind::Block {
                name: None,
                list: Vec::new(),
            },
            type_: Type::NONE,
        });
        let inner_switch_block = current_outer;

        let mut table = Vec::new();
        let mut switch_default_name = format!("{}$default", base);

        for (&_target_id, &branch_id) in &processed_branches {
            let branch = self.branches.get(branch_id).unwrap();
            if branch.switch_values.as_ref().map_or(true, |v| v.is_empty()) {
                switch_default_name = format!("{}$default", base);
                break;
            }
        }
        if switch_default_name.is_empty() {
            switch_default_name = format!("{}$default", base);
        }

        for (&target_id, &branch_id) in &processed_branches {
            let target_block_id_val = self.blocks.get(target_id).unwrap().id;
            let target_is_checked = self
                .blocks
                .get(target_id)
                .unwrap()
                .is_checked_multiple_entry;

            let (branch_has_switch_values, has_switch_values) = {
                let b = self.branches.get(branch_id).unwrap();
                (b.switch_values.is_some(), b.switch_values.clone())
            };
            let is_default = branch_has_switch_values
                && has_switch_values.as_ref().unwrap().is_empty()
                || !branch_has_switch_values;

            let mut curr_name = if is_default {
                format!("{}$default", base)
            } else {
                format!("{}$case${}", base, target_block_id_val)
            };

            let set_curr_label = set_label && target_is_checked;
            let has_fused_content = fused_info
                .as_ref()
                .map(|(_, m)| m.contains_key(&target_block_id_val))
                .unwrap_or(false);

            if has_fused_content {
                let branch_mut = self.branches.get_mut(branch_id).unwrap();
                branch_mut.branch_type = BranchType::Direct;
            }

            let mut curr_content = None;
            let (branch_type, branch_code) = {
                let b = self.branches.get(branch_id).unwrap();
                (b.branch_type, b.code)
            };

            if set_curr_label
                || branch_type != BranchType::Direct
                || has_fused_content
                || branch_code.is_some()
            {
                let mut content = self.render_branch(
                    builder,
                    label_var,
                    branch_id,
                    target_id,
                    block_map,
                    shape_map,
                    set_curr_label,
                    false, // multiple case is never a condition chain in this sense
                );
                if has_fused_content {
                    let (fused_m_id, fused_map) = fused_info.as_ref().unwrap();
                    let fused_shape_id = fused_map[&target_block_id_val];
                    let fused_next_stop = self.shapes.get(*fused_m_id).unwrap().next;
                    // Use more restrictive stop boundary
                    let target_stop = fused_next_stop.or(stop_at);
                    let fused_rendered = self.render_shape(
                        builder,
                        label_var,
                        fused_shape_id,
                        in_loop,
                        block_map,
                        shape_map,
                        target_stop,
                    );
                    content = builder.block(
                        None as Option<&str>,
                        vec![content, fused_rendered],
                        crate::ir::BlockType::Empty,
                    );
                }
                curr_content = Some(content);
            }

            if is_default {
                switch_default_name = curr_name.clone();
            }

            if let Some(content) = curr_content {
                let next_outer = builder.arena.alloc(Expression {
                    kind: ExpressionKind::Block {
                        name: None,
                        list: vec![current_outer, content],
                    },
                    type_: Type::NONE,
                });

                // Name the old outer so it can be jumped to
                if let ExpressionKind::Block { name, .. } = &mut builder.arena[current_outer].kind {
                    *name = Some(curr_name.clone());
                }

                if builder.arena[content].type_ != Type::UNREACHABLE {
                    let br = builder.arena.alloc(Expression {
                        kind: ExpressionKind::Break {
                            target: leave_id,
                            condition: None,
                            value: None,
                        },
                        type_: Type::NONE,
                    });
                    if let ExpressionKind::Block { list, .. } = &mut builder.arena[next_outer].kind
                    {
                        list.push(br);
                    }
                }
                current_outer = next_outer;
            } else {
                // No content, just go to leave
                let target_name = leave_name.clone();
                if is_default {
                    switch_default_name = target_name.clone();
                }
                // (Cases mapping will handle this via table)
                curr_name = target_name;
            }

            if let Some(values) = has_switch_values {
                for &val in &values {
                    while table.len() <= val as usize {
                        table.push(switch_default_name.clone());
                    }
                    table[val as usize] = curr_name.clone();
                }
            }
        }

        // Finalize leave block
        builder.arena[leave_id].kind = ExpressionKind::Block {
            name: Some(leave_name.clone()),
            list: vec![current_outer],
        };

        // If current_outer (or one of its ancestors) hasn't been named, and it's inner_switch_block,
        // name it with the default name so it can be targeted.
        if let ExpressionKind::Block { name, .. } = &mut builder.arena[inner_switch_block].kind {
            if name.is_none() {
                *name = Some(switch_default_name.clone());
            }
        }

        // Construct the actual switch
        let mut name_to_id = HashMap::new();
        let mut search_stack = vec![leave_id, inner_switch_block, current_outer];
        let mut visited = HashSet::new();
        while let Some(sid) = search_stack.pop() {
            if !visited.insert(sid) {
                continue;
            }
            let name = if let ExpressionKind::Block { name, .. } = &builder.arena[sid].kind {
                name.clone()
            } else if let ExpressionKind::If { name, .. } = &builder.arena[sid].kind {
                name.clone()
            } else if let ExpressionKind::Loop { name, .. } = &builder.arena[sid].kind {
                name.clone()
            } else {
                None
            };

            if let Some(n) = &name {
                name_to_id.insert(n.clone(), sid);
            }

            // Explicitly handle all IR types that can have children
            match &builder.arena[sid].kind {
                ExpressionKind::Block { list, .. } => {
                    for &child in list {
                        search_stack.push(child);
                    }
                }
                ExpressionKind::If {
                    if_true,
                    if_false,
                    condition,
                    ..
                } => {
                    search_stack.push(*if_true);
                    if let Some(f) = if_false {
                        search_stack.push(*f);
                    }
                    search_stack.push(*condition);
                }
                ExpressionKind::Loop { body, .. } => {
                    search_stack.push(*body);
                }
                ExpressionKind::Switch {
                    targets,
                    default,
                    condition,
                    ..
                } => {
                    for &t in targets {
                        search_stack.push(t);
                    }
                    search_stack.push(*default);
                    search_stack.push(*condition);
                }
                ExpressionKind::Unary { value, .. } => search_stack.push(*value),
                ExpressionKind::Binary { left, right, .. } => {
                    search_stack.push(*left);
                    search_stack.push(*right);
                }
                ExpressionKind::Select {
                    if_true,
                    if_false,
                    condition,
                } => {
                    search_stack.push(*if_true);
                    search_stack.push(*if_false);
                    search_stack.push(*condition);
                }
                ExpressionKind::LocalSet { value, .. } => search_stack.push(*value),
                ExpressionKind::LocalTee { value, .. } => search_stack.push(*value),
                ExpressionKind::GlobalSet { value, .. } => search_stack.push(*value),
                ExpressionKind::Load { ptr, .. } => search_stack.push(*ptr),
                ExpressionKind::Store { ptr, value, .. } => {
                    search_stack.push(*ptr);
                    search_stack.push(*value);
                }
                ExpressionKind::Call { operands, .. } => {
                    for &op in operands {
                        search_stack.push(op);
                    }
                }
                ExpressionKind::Drop { value } => search_stack.push(*value),
                ExpressionKind::Return { value } => {
                    if let Some(v) = value {
                        search_stack.push(*v);
                    }
                }
                ExpressionKind::Break {
                    condition, value, ..
                } => {
                    if let Some(c) = condition {
                        search_stack.push(*c);
                    }
                    if let Some(v) = value {
                        search_stack.push(*v);
                    }
                }
                _ => {
                    // Fallback for others (like Nop, Const, LocalGet etc have no children)
                    builder.arena[sid].kind.for_each_child(|child| {
                        search_stack.push(child);
                    });
                }
            }
        }

        let table_ids: Vec<ExpressionId> = table
            .iter()
            .map(|n| {
                *name_to_id
                    .get(n)
                    .unwrap_or_else(|| panic!("Switch target not found: {} for block {}", n, base))
            })
            .collect();
        let default_id = *name_to_id.get(&switch_default_name).unwrap_or_else(|| {
            panic!(
                "Switch default not found: {} for block {}",
                switch_default_name, base
            )
        });

        let switch_expr = builder.switch(table_ids, default_id, condition, None);
        // Put switch inside the inner_switch_block
        if let ExpressionKind::Block { list, .. } = &mut builder.arena[inner_switch_block].kind {
            list.push(switch_expr);
        }

        list.push(leave_id);
        builder.block(None as Option<&str>, list, crate::ir::BlockType::Empty)
    }

    fn render_branch(
        &mut self,
        builder: &mut IrBuilder,
        label_var: LocalId,
        branch_id: RelooperBranchId,
        target_id: RelooperBlockId,
        block_map: &std::collections::HashMap<u32, ExpressionId>,
        shape_map: &std::collections::HashMap<u32, ExpressionId>,
        set_label: bool,
        is_in_if_chain: bool,
    ) -> ExpressionId {
        let (branch_code, branch_type, branch_ancestor) = {
            let branch = self.branches.get(branch_id).unwrap();
            (branch.code, branch.branch_type, branch.ancestor)
        };
        let (target_id_val, target_expr_id) = {
            let target = self.blocks.get(target_id).unwrap();
            (target.id, block_map[&target.id])
        };
        let mut list = Vec::new();

        if let Some(code) = branch_code {
            list.push(code);
        }

        if set_label {
            list.push(self.make_set_label(builder, label_var, target_id_val));
        }

        match branch_type {
            BranchType::Direct => {
                if is_in_if_chain {
                    let br = builder.arena.alloc(Expression {
                        kind: ExpressionKind::Break {
                            target: target_expr_id,
                            condition: None,
                            value: None,
                        },
                        type_: Type::NONE,
                    });
                    list.push(br);
                }
            }
            BranchType::Break => {
                let br = builder.arena.alloc(Expression {
                    kind: ExpressionKind::Break {
                        target: target_expr_id,
                        condition: None,
                        value: None,
                    },
                    type_: Type::NONE,
                });
                list.push(br);
            }
            BranchType::Continue => {
                let ancestor_id = branch_ancestor.unwrap();
                let ancestor_id_val = self.shapes.get(ancestor_id).unwrap().id;
                let ancestor_expr_id = shape_map[&ancestor_id_val];
                let br = builder.arena.alloc(Expression {
                    kind: ExpressionKind::Break {
                        target: ancestor_expr_id,
                        condition: None,
                        value: None,
                    },
                    type_: Type::NONE,
                });
                list.push(br);
            }
        }

        builder.block(None as Option<&str>, list, crate::ir::BlockType::Empty)
    }

    fn make_check_label(
        &self,
        builder: &mut IrBuilder,
        label_var: LocalId,
        id: u32,
    ) -> ExpressionId {
        let get = builder.local_get(label_var, Type::I32);
        let val = builder.const_(Literal::I32(id as i32));
        builder.binary(crate::ir::ops::BinaryOp::EqInt32, get, val, Type::I32)
    }

    fn make_set_label(&self, builder: &mut IrBuilder, label_var: LocalId, id: u32) -> ExpressionId {
        let val = builder.const_(Literal::I32(id as i32));
        builder.local_set(label_var, val)
    }

    fn expr_contains(&self, builder: &IrBuilder, root: ExpressionId, target: ExpressionId) -> bool {
        let mut stack = vec![root];
        while let Some(eid) = stack.pop() {
            if eid == target {
                return true;
            }
            // Push children for traversal
            match &builder.arena[eid].kind {
                ExpressionKind::Block { list, .. } => {
                    for &c in list {
                        stack.push(c);
                    }
                }
                kind => {
                    kind.for_each_child(|child| stack.push(child));
                }
            }
        }
        false
    }

    fn handle_followup_multiples(
        &mut self,
        builder: &mut IrBuilder,
        label_var: LocalId,
        ret: ExpressionId,
        shape_id: ShapeId,
        in_loop: bool,
        block_map: &std::collections::HashMap<u32, ExpressionId>,
        shape_map: &std::collections::HashMap<u32, ExpressionId>,
        stop_at: Option<ShapeId>,
    ) -> ExpressionId {
        let mut curr_expr = ret;

        // while Parent->Next is Multiple
        while let Some(next_id) = self.shapes.get(shape_id).unwrap().next {
            if Some(next_id) == stop_at {
                break;
            }
            let next_shape_kind = self.shapes.get(next_id).unwrap().kind.clone();
            if let ShapeKind::Multiple { inner_map } = next_shape_kind {
                // Consume it
                let next_next = self.shapes.get(next_id).unwrap().next;
                self.shapes.get_mut(shape_id).unwrap().next = next_next;

                let mut sorted_ids: Vec<_> = inner_map.keys().cloned().collect();
                sorted_ids.sort();

                let mut list = vec![curr_expr];
                for id in sorted_ids {
                    let body_shape_id = inner_map[&id];
                    let condition = self.make_check_label(builder, label_var, id);
                    let body_rendered = self.render_shape(
                        builder,
                        label_var,
                        body_shape_id,
                        in_loop,
                        block_map,
                        shape_map,
                        stop_at,
                    );
                    list.push(builder.if_(
                        None as Option<&str>,
                        condition,
                        body_rendered,
                        None,
                        crate::ir::BlockType::Empty,
                    ));
                }
                curr_expr = builder.block(None as Option<&str>, list, crate::ir::BlockType::Empty);
            } else {
                break;
            }
        }

        // after the multiples is a simple or a loop, in both cases we must hit an
        // entry block, and so this is the last one we need to take into account now
        if let Some(next_id) = self.shapes.get(shape_id).unwrap().next {
            let next_shape_kind = self.shapes.get(next_id).unwrap().kind.clone();
            match next_shape_kind {
                ShapeKind::Simple { inner } => {
                    let target_block_id = self.blocks.get(inner).unwrap().id;
                    let vocab_id = block_map[&target_block_id];
                    if let ExpressionKind::Block { name, .. } = &mut builder.arena[vocab_id].kind {
                        if name.is_none() {
                            *name = Some(format!("block$break${}", target_block_id));
                        }
                    }
                }
                ShapeKind::Loop { entries, .. } => {
                    for entry_id in entries {
                        let target_block_id = self.blocks.get(entry_id).unwrap().id;
                        let vocab_id = block_map[&target_block_id];
                        if let ExpressionKind::Block { name, .. } =
                            &mut builder.arena[vocab_id].kind
                        {
                            if name.is_none() {
                                *name = Some(format!("block$break${}", target_block_id));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        curr_expr
    }
}
