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

#[derive(Debug)]
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
        // 1. Liveness
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

        // 2. Incoming branches (restricted to live blocks)
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

        // 3. Process
        self.optimize(entry, arena);

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

    fn skip_empty_blocks(&mut self, arena: &IdArena<ExpressionMarker, Expression>) -> bool {
        let mut worked = false;
        let block_ids: Vec<_> = self.blocks.iter().map(|(id, _)| id).collect();

        for source_id in block_ids {
            let mut changed = false;
            let mut new_branches_out = HashMap::new();

            // We need to clone or extract because we'll mutate self.blocks
            let current_branches: Vec<_> = {
                let block = self.blocks.get(source_id).unwrap();
                block.branches_out.iter().map(|(&t, &b)| (t, b)).collect()
            };

            for (target_id, branch_id) in current_branches {
                let mut replacement_id = target_id;
                let mut seen = HashSet::new();
                seen.insert(target_id);

                while self.is_block_empty(replacement_id, arena) {
                    let next_block = self.blocks.get(replacement_id).unwrap();
                    if next_block.branches_out.len() == 1 {
                        let (&next_next_id, &next_next_branch_id) =
                            next_block.branches_out.iter().next().unwrap();
                        let next_next_branch = self.branches.get(next_next_branch_id).unwrap();

                        if next_next_branch.condition.is_none()
                            && next_next_branch.code.is_none()
                            && next_next_branch.switch_values.is_none()
                        {
                            if !seen.insert(next_next_id) {
                                break;
                            }
                            replacement_id = next_next_id;
                            changed = true;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }

                if replacement_id != target_id {
                    worked = true;
                }

                // If collision, we just keep the existing branch ID (simple skip)
                if !new_branches_out.contains_key(&replacement_id) {
                    new_branches_out.insert(replacement_id, branch_id);
                }
            }

            if changed {
                self.blocks.get_mut(source_id).unwrap().branches_out = new_branches_out;
            }
        }
        worked
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

            if num_predecessors.get(&target_id).copied().unwrap_or(0) == 1 {
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
            let block = self.blocks.get(inner_id).unwrap();
            for &target_id in block.branches_out.keys() {
                if blocks.contains(&target_id) {
                    next_entries.insert(target_id);
                }
            }

            let mut just_inner = HashSet::new();
            just_inner.insert(inner_id);
            let next_entries_vec: Vec<_> = next_entries.iter().cloned().collect();
            for &next_id in &next_entries_vec {
                self.solipsize(next_id, BranchType::Break, shape_id, &just_inner);
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
        let mut inner_blocks = HashSet::new();
        let mut queue: Vec<_> = entries.iter().cloned().collect();

        while let Some(curr_id) = queue.pop() {
            if !inner_blocks.contains(&curr_id) {
                inner_blocks.insert(curr_id);
                blocks.remove(&curr_id);
                let block = self.blocks.get(curr_id).unwrap();
                for &prev_id in &block.branches_in {
                    queue.push(prev_id);
                }
            }
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
            id: 0,
            kind: ShapeKind::Simple {
                inner: *entries.iter().next().unwrap(),
            }, // Dummy, will be replaced
            next: None,
        });

        let shape_id = self.shapes.alloc(Shape {
            id: self.shape_id_counter,
            kind: ShapeKind::Loop {
                inner: placeholder_inner,
                entries: entries.clone(),
            },
            next: None,
        });
        self.shape_id_counter += 1;

        for &entry_id in &entries {
            self.solipsize(entry_id, BranchType::Continue, shape_id, &inner_blocks);
        }
        let next_entries_vec: Vec<_> = next_entries.iter().cloned().collect();
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
                        // In Binaryen we check if parent is in the "current" set.
                        // Here we just check ownership.
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
                self.solipsize(target_id, BranchType::Break, shape_id, &group_blocks);
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
                if block.branches_in.is_empty() {
                    current_shape_id =
                        Some(self.make_simple(&mut blocks, curr_id, &mut next_entries));
                } else {
                    current_shape_id =
                        Some(self.make_loop(&mut blocks, entries.clone(), &mut next_entries));
                }
            } else {
                let mut independent_groups = HashMap::new();
                self.find_independent_groups(&entries, &mut independent_groups);

                if !independent_groups.is_empty() {
                    // Check if entries can be reached by other groups (Multiple)
                    let mut validated_groups = HashMap::new();
                    for (&entry_id, group) in &independent_groups {
                        let mut valid = true;
                        let block = self.blocks.get(entry_id).unwrap();
                        for &prior_id in &block.branches_in {
                            if !group.contains(&prior_id) {
                                // Reached from outside the group
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

    pub fn render(&self, builder: &mut IrBuilder, label_var: LocalId) -> ExpressionId {
        self.rendered_shapes.borrow_mut().clear();
        self.rendered_blocks.borrow_mut().clear();
        let mut block_map = std::collections::HashMap::new();
        let mut shape_map = std::collections::HashMap::new();

        for (_, block) in &self.blocks {
            let expr = builder.arena.alloc(Expression {
                kind: ExpressionKind::Nop,
                type_: Type::NONE,
            });
            block_map.insert(block.id, expr);
        }
        for (_, shape) in &self.shapes {
            let expr = builder.arena.alloc(Expression {
                kind: ExpressionKind::Nop,
                type_: Type::NONE,
            });
            shape_map.insert(shape.id, expr);
        }

        if let Some(root_id) = self.root {
            self.render_shape(builder, label_var, root_id, false, &block_map, &shape_map)
        } else {
            builder.nop()
        }
    }

    fn render_shape(
        &self,
        builder: &mut IrBuilder,
        label_var: LocalId,
        shape_id: ShapeId,
        in_loop: bool,
        block_map: &std::collections::HashMap<u32, ExpressionId>,
        shape_map: &std::collections::HashMap<u32, ExpressionId>,
    ) -> ExpressionId {
        let shape = self.shapes.get(shape_id).unwrap();

        // Avoid re-rendering shapes we've already done
        if let ShapeKind::Simple { inner } = &shape.kind {
            if self.rendered_blocks.borrow().contains(inner) {
                return block_map[&self.blocks[*inner].id];
            }
        } else {
            if self.rendered_shapes.borrow().contains(&shape_id) {
                return shape_map[&shape.id];
            }
        }
        self.rendered_shapes.borrow_mut().insert(shape_id);

        let mut ret = match &shape.kind {
            ShapeKind::Simple { inner } => {
                self.rendered_blocks.borrow_mut().insert(*inner);
                let block = self.blocks.get(*inner).unwrap();
                let vocab_id = block_map[&block.id];
                let rendered =
                    self.render_block(builder, label_var, *inner, in_loop, block_map, shape_map);

                // Update the pre-allocated ID with the rendered content
                // Preserve any existing content (e.g. from handle_followup_multiples)
                if matches!(builder.arena[vocab_id].kind, ExpressionKind::Nop) {
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
                        name: Some(format!("block$break${}", block.id)),
                        list: new_list,
                    };
                }
                vocab_id
            }
            ShapeKind::Multiple { inner_map } => {
                let mut sorted_keys: Vec<_> = inner_map.keys().cloned().collect();
                sorted_keys.sort();
                let shape_id = shape.id;
                let vocab_id = shape_map[&shape_id];
                let rendered = self.render_multiple_recursive(
                    builder,
                    label_var,
                    &sorted_keys,
                    inner_map,
                    0,
                    in_loop,
                    block_map,
                    shape_map,
                );
                // Multiples use the pre-allocated shape ID. Merge if already has content.
                if matches!(builder.arena[vocab_id].kind, ExpressionKind::Nop) {
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
                        name: Some(format!("block$break${}", shape_id)),
                        list: new_list,
                    };
                }
                vocab_id
            }
            ShapeKind::Loop { inner, .. } => {
                let body =
                    self.render_shape(builder, label_var, *inner, true, block_map, shape_map);
                let loop_id = shape_map[&shape.id];
                builder.arena[loop_id] = Expression {
                    kind: ExpressionKind::Loop {
                        name: Some(format!("shape$continue${}", shape.id)),
                        body,
                    },
                    type_: Type::from(builder.arena[body].type_),
                };
                loop_id
            }
        };

        ret = self.handle_followup_multiples(
            builder, label_var, ret, shape_id, in_loop, block_map, shape_map,
        );

        if let Some(next_id) = shape.next {
            let next_expr =
                self.render_shape(builder, label_var, next_id, in_loop, block_map, shape_map);
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
        &self,
        builder: &mut IrBuilder,
        label_var: LocalId,
        keys: &[u32],
        inner_map: &HashMap<u32, ShapeId>,
        index: usize,
        in_loop: bool,
        block_map: &std::collections::HashMap<u32, ExpressionId>,
        shape_map: &std::collections::HashMap<u32, ExpressionId>,
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
        &self,
        builder: &mut IrBuilder,
        label_var: LocalId,
        block_id: RelooperBlockId,
        in_loop: bool,
        block_map: &std::collections::HashMap<u32, ExpressionId>,
        shape_map: &std::collections::HashMap<u32, ExpressionId>,
    ) -> ExpressionId {
        let block = self.blocks.get(block_id).unwrap();
        let mut list = Vec::new();

        if block.is_checked_multiple_entry && in_loop {
            list.push(self.make_set_label(builder, label_var, 0));
        }

        let block_expr = builder.arena.get(block.code).unwrap();
        if let ExpressionKind::Block {
            list: inner_list, ..
        } = &block_expr.kind
        {
            list.extend(inner_list.iter().cloned());
        } else {
            list.push(block.code);
        }

        if let Some(switch_cond) = block.switch_condition {
            return self.render_switch(
                builder,
                label_var,
                block_id,
                switch_cond,
                list,
                in_loop,
                block_map,
                shape_map,
            );
        }

        if block.processed_branches_out.is_empty() {
            let ty = list
                .last()
                .map(|&id| builder.arena[id].type_)
                .unwrap_or(Type::NONE);
            return builder.block(None as Option<&str>, list, crate::ir::BlockType::from(ty));
        }

        let mut branches_to_render = Vec::new();
        let mut default_target = None;

        for (&target_id, &branch_id) in &block.processed_branches_out {
            let branch = self.branches.get(branch_id).unwrap();
            if branch.condition.is_none() && branch.switch_values.is_none() {
                default_target = Some((target_id, branch_id));
            } else {
                branches_to_render.push((target_id, branch_id));
            }
        }

        let mut root = if let Some((target_id, branch_id)) = default_target {
            self.render_branch(
                builder, label_var, branch_id, target_id, block_map, shape_map,
            )
        } else {
            builder.nop()
        };

        for (target_id, branch_id) in branches_to_render {
            let branch = self.branches.get(branch_id).unwrap();
            let branch_render = self.render_branch(
                builder, label_var, branch_id, target_id, block_map, shape_map,
            );
            if let Some(cond) = branch.condition {
                root = builder.if_(
                    None as Option<&str>,
                    cond,
                    branch_render,
                    Some(root),
                    crate::ir::BlockType::Empty,
                );
            }
        }

        list.push(root);
        builder.block(None as Option<&str>, list, crate::ir::BlockType::Empty)
    }

    fn render_switch(
        &self,
        builder: &mut IrBuilder,
        label_var: LocalId,
        block_id: RelooperBlockId,
        condition: ExpressionId,
        mut list: Vec<ExpressionId>,
        _in_loop: bool,
        block_map: &std::collections::HashMap<u32, ExpressionId>,
        shape_map: &std::collections::HashMap<u32, ExpressionId>,
    ) -> ExpressionId {
        let block = self.blocks.get(block_id).unwrap();
        let base = format!("switch${}", block.id);
        let default_name = format!("{}$default", base);
        let leave_name = format!("{}$leave", base);

        let mut table = Vec::new();
        let mut case_bodies = Vec::new();
        let mut default_target_label = default_name.clone();

        for (&target_id, &branch_id) in &block.processed_branches_out {
            let branch = self.branches.get(branch_id).unwrap();
            let label = if branch.switch_values.is_some() {
                format!("{}$case${}", base, target_id)
            } else {
                default_name.clone()
            };

            let content = self.render_branch(
                builder, label_var, branch_id, target_id, block_map, shape_map,
            );
            case_bodies.push((label.clone(), content));

            if let Some(values) = &branch.switch_values {
                for &val in values {
                    while table.len() <= val as usize {
                        table.push(default_name.clone());
                    }
                    table[val as usize] = label.clone();
                }
            } else {
                default_target_label = label;
            }
        }

        // Reserve IDs for labels
        let mut label_map: std::collections::HashMap<String, ExpressionId> =
            std::collections::HashMap::new();
        let mut all_labels = std::collections::HashSet::new();
        for t in &table {
            all_labels.insert(t.clone());
        }
        all_labels.insert(default_target_label.clone());
        for (l, _) in &case_bodies {
            all_labels.insert(l.clone());
        }

        for lab in all_labels {
            let id = builder.arena.alloc(Expression {
                kind: ExpressionKind::Nop,
                type_: Type::NONE,
            });
            label_map.insert(lab, id);
        }

        // Reserve ID for leave block
        let leave_id = builder.arena.alloc(Expression {
            kind: ExpressionKind::Nop,
            type_: Type::NONE,
        });

        let table_ids: Vec<ExpressionId> = table.iter().map(|t| label_map[t]).collect();
        let default_id = label_map[&default_target_label];

        let mut res = builder.switch(table_ids, default_id, condition, None);

        for (label, content) in case_bodies {
            let inner_id = label_map[&label];

            // Overwrite inner_id with Block(label, [res])
            builder.arena[inner_id] = Expression {
                kind: ExpressionKind::Block {
                    name: Some(label),
                    list: vec![res],
                },
                type_: Type::NONE,
            };

            // Break to leave
            let br = builder.arena.alloc(Expression {
                kind: ExpressionKind::Break {
                    target: leave_id,
                    condition: None,
                    value: None,
                },
                type_: Type::NONE,
            });

            res = builder.block(
                None as Option<&str>,
                vec![inner_id, content, br],
                crate::ir::BlockType::Empty,
            );
        }

        // Overwrite leave_id with final block
        builder.arena[leave_id] = Expression {
            kind: ExpressionKind::Block {
                name: Some(leave_name),
                list: vec![res],
            },
            type_: Type::NONE,
        };
        res = leave_id;

        list.push(res);
        builder.block(None as Option<&str>, list, crate::ir::BlockType::Empty)
    }

    fn render_branch(
        &self,
        builder: &mut IrBuilder,
        label_var: LocalId,
        branch_id: RelooperBranchId,
        target_id: RelooperBlockId,
        block_map: &std::collections::HashMap<u32, ExpressionId>,
        shape_map: &std::collections::HashMap<u32, ExpressionId>,
    ) -> ExpressionId {
        let branch = self.branches.get(branch_id).unwrap();
        let target = self.blocks.get(target_id).unwrap();
        let mut list = Vec::new();

        if let Some(code) = branch.code {
            list.push(code);
        }

        if target.is_checked_multiple_entry {
            list.push(self.make_set_label(builder, label_var, target.id));
        }

        match branch.branch_type {
            BranchType::Direct => {}
            BranchType::Break => {
                let target_expr_id = block_map[&target.id];
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
                let ancestor_id = branch.ancestor.unwrap();
                let ancestor = self.shapes.get(ancestor_id).unwrap();
                let ancestor_expr_id = shape_map[&ancestor.id];
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
        &self,
        builder: &mut IrBuilder,
        _label_var: LocalId,
        ret: ExpressionId,
        shape_id: ShapeId,
        _in_loop: bool,
        block_map: &std::collections::HashMap<u32, ExpressionId>,
        _shape_map: &std::collections::HashMap<u32, ExpressionId>,
    ) -> ExpressionId {
        let shape = self.shapes.get(shape_id).unwrap();
        if shape.next.is_none() {
            return ret;
        }

        let mut curr = ret;
        let mut next_id = shape.next;

        while let Some(sid) = next_id {
            let next_shape = self.shapes.get(sid).unwrap();
            match &next_shape.kind {
                ShapeKind::Multiple { inner_map } => {
                    for (&id, _) in inner_map {
                        let vocab_id = block_map[&id];
                        match &builder.arena[vocab_id].kind {
                            ExpressionKind::Block { list, .. }
                                if list.len() == 1 && list[0] == curr =>
                            {
                                // Already wrapping the same `curr`, reuse it to avoid duplicate blocks
                                curr = vocab_id;
                            }
                            _ => {
                                // Preserve any existing content in the vocab block when wrapping
                                let mut new_list: Vec<ExpressionId> = Vec::new();
                                // We need to handle the case where vocab_id already has content
                                if !matches!(builder.arena[vocab_id].kind, ExpressionKind::Nop) {
                                    if let ExpressionKind::Block { list, .. } =
                                        &builder.arena[vocab_id].kind
                                    {
                                        for &e in list {
                                            if e != curr {
                                                new_list.push(e);
                                            }
                                        }
                                    } else {
                                        // Wrap the non-block existing content
                                        let old_content = builder.arena.alloc(Expression {
                                            kind: builder.arena[vocab_id].kind.clone(),
                                            type_: builder.arena[vocab_id].type_,
                                        });
                                        new_list.push(old_content);
                                    }
                                }

                                // Order: curr (preceding code) FIRST, then new join-point code.
                                // Actually, handle_followup_multiples is for joins where curr branches to id.
                                // So curr should be BEFORE the instructions in the block.
                                new_list.insert(0, curr);

                                builder.arena[vocab_id] = Expression {
                                    kind: ExpressionKind::Block {
                                        name: Some(format!("block$break${}", id)),
                                        list: new_list,
                                    },
                                    type_: Type::NONE,
                                };
                                curr = vocab_id;
                            }
                        }
                    }
                    next_id = next_shape.next;
                }
                _ => {
                    // Simple and Loop shapes just follow and are handled by the next_id
                    // logic at the end of render_shape.
                    break;
                }
            }
        }
        curr
    }
}
