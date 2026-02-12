use crate::ir::expression::{ExpressionId, ExpressionKind};
use crate::ir::module::{Function, FunctionKind, Module};
use crate::ir::{Literal, Type};

/// The RustPrinter converts the annotated IR into human-readable Rust code.
pub struct RustPrinter<'m> {
    pub module: &'m Module,
    output: String,
    indent: usize,
    loop_stack: Vec<LoopContext>,
    find_all_targets: std::collections::HashSet<ExpressionId>,
    wrapped_targets: std::collections::HashSet<ExpressionId>,
}

#[derive(Clone, Copy)]
struct LoopContext {
    break_target: Option<ExpressionId>,
    continue_target: Option<ExpressionId>,
}

impl<'m> RustPrinter<'m> {
    pub fn new(module: &'m Module) -> Self {
        Self {
            module,
            output: String::new(),
            indent: 0,
            loop_stack: Vec::new(),
            find_all_targets: std::collections::HashSet::new(),
            wrapped_targets: std::collections::HashSet::new(),
        }
    }

    fn collect_targets(&mut self, _func: &Function, expr_id: ExpressionId) {
        struct TargetCollector<'a> {
            targets: &'a mut std::collections::HashSet<ExpressionId>,
        }
        impl<'a> crate::ir::visitor::ReadOnlyVisitor for TargetCollector<'a> {
            fn visit_expression(
                &mut self,
                expr: ExpressionId,
                arena: &crate::collections::id_arena::IdArena<
                    crate::ir::expression::ExpressionMarker,
                    crate::ir::expression::Expression,
                >,
            ) {
                if let ExpressionKind::Break { target, .. } = &arena[expr].kind {
                    self.targets.insert(*target);
                }
                if let ExpressionKind::Switch {
                    targets, default, ..
                } = &arena[expr].kind
                {
                    for &t in targets {
                        self.targets.insert(t);
                    }
                    self.targets.insert(*default);
                }
                self.visit_children(expr, arena);
            }
        }
        let mut collector = TargetCollector {
            targets: &mut self.find_all_targets,
        };
        use crate::ir::visitor::ReadOnlyVisitor;
        collector.visit(expr_id, &self.module.expressions);
    }

    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
    }

    fn sanitize_label(name: &str) -> String {
        let name = name
            .replace("block_break_", "L")
            .replace("block_break", "L")
            .replace(".", "_")
            .replace("$", "_");

        let mut out = String::new();
        for c in name.chars() {
            if c.is_ascii_alphanumeric() || c == '_' {
                out.push(c);
            } else {
                out.push('_');
            }
        }
        // Label must not start with a digit
        if out.chars().next().map_or(false, |c| c.is_ascii_digit()) {
            out = format!("L{}", out);
        }
        out
    }

    fn render_boolean_condition(&mut self, func: &Function, condition: ExpressionId) {
        // Prefer printing boolean constants as `true`/`false` when used as conditions
        match &self.module.expressions[condition].kind {
            ExpressionKind::Const(lit) => {
                use crate::ir::Literal;
                match lit {
                    Literal::I32(v) => self.output.push_str(if *v != 0 { "true" } else { "false" }),
                    Literal::I64(v) => self.output.push_str(if *v != 0 { "true" } else { "false" }),
                    _ => self.walk_inline_expression(func, condition),
                }
            }
            _ => {
                self.walk_inline_expression(func, condition);
            }
        }
    }

    fn emit_block_label_if_needed(
        &mut self,
        _func: &Function,
        expr_id: ExpressionId,
        name: &Option<String>,
    ) -> bool {
        if self.find_all_targets.contains(&expr_id) && !self.wrapped_targets.contains(&expr_id) {
            let label = if let Some(n) = name {
                Self::sanitize_label(n)
            } else {
                format!("L{}", expr_id.index())
            };
            self.output.push_str(&format!("'{}: ", label));
            true
        } else {
            false
        }
    }

    fn get_local_name(
        &self,
        func: &Function,
        id: crate::ir::LocalId,
        expr: Option<ExpressionId>,
    ) -> String {
        if let FunctionKind::Local { locals, .. } = &func.kind {
            if let Some(local) = locals.get(id) {
                if !local.name.is_empty() {
                    return local.name.clone();
                }
            }
        }

        if let Some(e) = expr {
            if let Some(name) = self.module.annotations.get_local_name(e) {
                return name.to_string();
            }
        }
        format!("v{}", id.index())
    }

    fn is_last_in_func(&self, func: &Function, expr_id: ExpressionId) -> bool {
        if let FunctionKind::Local { body, .. } = &func.kind {
            self.is_last_in_expr(*body, expr_id)
        } else {
            false
        }
    }

    fn is_last_in_expr(&self, parent: ExpressionId, expr_id: ExpressionId) -> bool {
        if parent == expr_id {
            return true;
        }
        match &self.module.expressions[parent].kind {
            ExpressionKind::Block { list, .. } => {
                if let Some(&last) = list.last() {
                    return self.is_last_in_expr(last, expr_id);
                }
            }
            ExpressionKind::If {
                if_true, if_false, ..
            } => {
                if self.is_last_in_expr(*if_true, expr_id) {
                    return true;
                }
                if let Some(f) = if_false {
                    if self.is_last_in_expr(*f, expr_id) {
                        return true;
                    }
                }
            }
            _ => {}
        }
        false
    }

    pub fn print(&mut self) -> String {
        self.output.clear();
        self.output
            .push_str("// Decompiled to Rust from WebAssembly\n// MARKER_CHECK\n\n");

        self.print_types();

        for (_, func) in &self.module.functions {
            self.print_function(func);
        }

        self.output.clone()
    }

    fn print_types(&mut self) {
        if self.module.type_names.is_empty() {
            return;
        }

        self.output.push_str("// Types:\n");
        for (i, name) in self.module.type_names.iter().enumerate() {
            if let Some((_, ty)) = self.module.types.iter().nth(i) {
                let params = self.format_type_list(ty.params);
                let results = self.format_type_list(ty.results);
                self.output
                    .push_str(&format!("//   {} : ({}) -> ({})\n", name, params, results));
            }
        }
        self.output.push_str("\n");
    }

    fn format_type_list(&self, ty: Type) -> String {
        let elements = ty.tuple_elements();
        if elements.is_empty() {
            return "()".to_string();
        }
        elements
            .iter()
            .map(|&t| self.type_name(t))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn type_name(&self, ty: Type) -> String {
        if ty == Type::NONE {
            return "()".to_string();
        }
        if ty == Type::I32 {
            return "i32".to_string();
        }
        if ty == Type::I64 {
            return "i64".to_string();
        }
        if ty == Type::F32 {
            return "f32".to_string();
        }
        if ty == Type::F64 {
            return "f64".to_string();
        }
        if ty == Type::V128 {
            return "v128".to_string();
        }

        if ty.is_signature() {
            if let Some(sig) = crate::ir::type_store::try_lookup_signature(ty) {
                return format!(
                    "fn({}) -> {}",
                    self.format_type_list(sig.params),
                    self.format_type_list(sig.results)
                );
            }
        }

        if ty.is_tuple() {
            return format!("({})", self.format_type_list(ty));
        }

        if ty.is_ref() {
            let nullable = if ty.is_nullable() { "Option<" } else { "" };
            let suffix = if ty.is_nullable() { ">" } else { "" };
            let inner = if let Some(ht) = ty.get_heap_type() {
                match ht {
                    crate::ir::HeapType::FUNC => "FuncRef".to_string(),
                    crate::ir::HeapType::EXT => "ExternRef".to_string(),
                    crate::ir::HeapType::ANY => "AnyRef".to_string(),
                    crate::ir::HeapType::EQ => "EqRef".to_string(),
                    crate::ir::HeapType::I31 => "i31".to_string(),
                    crate::ir::HeapType::STRUCT => "StructRef".to_string(),
                    crate::ir::HeapType::ARRAY => "ArrayRef".to_string(),
                    _ => format!("{:?}", ht),
                }
            } else {
                "Ref".to_string()
            };
            return format!("{}{}{}", nullable, inner, suffix);
        }

        format!("{:?}", ty).to_lowercase()
    }

    fn walk_block_list(
        &mut self,
        func: &Function,
        block_id: ExpressionId,
        list: &[ExpressionId],
        start_idx: usize,
    ) {
        // Successor wrapping for Rust forward jumps (jumps to a sibling in a block)
        // If a child (or something at the start of it) is a target for a break,
        // we wrap preceding siblings in a labeled block.
        let mut targets = Vec::new();
        for (idx, &child_id) in list.iter().enumerate() {
            for &target_id in &self.find_all_targets {
                // Only wrap if we can skip some preceding siblings (idx > 0)
                // and the target is at the start of this child.
                if idx > 0 && self.is_at_start(child_id, target_id) {
                    targets.push((idx, target_id));
                }
            }
        }
        // Sort by child index (ascending)
        targets.sort_unstable_by_key(|t| (t.0, t.1.index()));

        let block_has_value = self.module.expressions[block_id].type_ != Type::NONE
            && self.module.expressions[block_id].type_ != Type::UNREACHABLE;

        // Open wrappers: from last target (outermost) to first (innermost)
        for &(_idx, target_id) in targets.iter().rev() {
            self.wrapped_targets.insert(target_id);
            let name = match &self.module.expressions[target_id].kind {
                ExpressionKind::Block { name, .. }
                | ExpressionKind::If { name, .. }
                | ExpressionKind::Loop { name, .. } => name.clone(),
                _ => None,
            };
            let label = if let Some(n) = name {
                Self::sanitize_label(&n)
            } else {
                format!("L{}", target_id.index())
            };
            self.write_indent();
            self.output.push_str(&format!("'{}: {{\n", label));
            self.indent += 1;
        }

        // For closing, we want to pop them back in the order they were opened
        targets.reverse();

        for (i, &child_id) in list.iter().enumerate().skip(start_idx) {
            // If we reached a target index, close all wrappers associated with it
            while !targets.is_empty() && targets.last().unwrap().0 <= i {
                targets.pop();
                self.indent -= 1;
                self.write_indent();
                self.output.push_str("}\n");
            }

            let child_is_last = i == list.len() - 1;
            let child_is_statement = !(child_is_last && block_has_value);

            let mut pushed_ctx = None;
            if let ExpressionKind::Loop { .. } = &self.module.expressions[child_id].kind {
                if child_is_last {
                    pushed_ctx = Some(LoopContext {
                        break_target: Some(block_id),
                        continue_target: Some(child_id),
                    });
                    self.loop_stack.push(pushed_ctx.unwrap());
                }
            }

            self.walk_expression_ext(func, child_id, child_is_statement);

            if pushed_ctx.is_some() {
                self.loop_stack.pop();
            }
        }

        // Close any remaining wrappers
        while !targets.is_empty() {
            targets.pop();
            self.indent -= 1;
            self.write_indent();
            self.output.push_str("}\n");
        }
    }

    fn print_function(&mut self, func: &Function) {
        self.find_all_targets.clear();
        self.wrapped_targets.clear();
        if let FunctionKind::Local { body, .. } = &func.kind {
            self.collect_targets(func, *body);
        }

        self.output.push_str(&format!("fn {}(", func.name));

        let param_types = func.params.tuple_elements();
        if let FunctionKind::Local { param_ids, .. } = &func.kind {
            for (i, &ty) in param_types.iter().enumerate() {
                if i > 0 {
                    self.output.push_str(", ");
                }
                let name = if i < param_ids.len() {
                    self.get_local_name(func, param_ids[i], None)
                } else {
                    format!("p{}", i)
                };
                self.output
                    .push_str(&format!("{}: {}", name, self.type_name(ty)));
            }
        } else {
            // Import or other
            for (i, &ty) in param_types.iter().enumerate() {
                if i > 0 {
                    self.output.push_str(", ");
                }
                self.output
                    .push_str(&format!("p{}: {}", i, self.type_name(ty)));
            }
        }

        self.output.push_str(")");

        if func.results != Type::NONE {
            self.output.push_str(" -> ");
            self.output.push_str(&self.type_name(func.results));
        }

        match &func.kind {
            FunctionKind::Local {
                body,
                locals,
                var_ids,
                ..
            } => {
                self.output.push_str(" {\n");
                self.indent += 1;

                // Declare locals
                for &id in var_ids {
                    let name = self.get_local_name(func, id, None);
                    let ty = locals[id].type_;
                    let default = match ty {
                        Type::I32 | Type::I64 => "0",
                        Type::F32 | Type::F64 => "0.0",
                        _ => "0",
                    };
                    self.write_indent();
                    self.output.push_str(&format!(
                        "let mut {}: {} = {};\n",
                        name,
                        self.type_name(ty),
                        default
                    ));
                }

                // If the body is a block, we only walk its contents to avoid
                // extra braces { { body } }.
                if let ExpressionKind::Block { list, .. } = &self.module.expressions[*body].kind {
                    self.walk_block_list(func, *body, list, 0);
                } else {
                    self.walk_expression_ext(func, *body, true);
                }

                self.indent -= 1;
                self.output.push_str("}\n\n");
            }
            FunctionKind::Import(import) => {
                self.output.push_str(&format!(
                    "; // import from {}.{}\n\n",
                    import.module, import.name
                ));
            }
        }
    }

    fn print_load(
        &mut self,
        func: &Function,
        _expr_id: ExpressionId,
        ptr: ExpressionId,
        offset: u32,
        bytes: u32,
        is_inline: bool,
    ) {
        let mut handled = false;

        // Try to handle as struct access via annotations
        if let Some(ann) = self.module.annotations.get(ptr) {
            if let Some(offsets) = &ann.struct_offsets {
                if offsets.contains(&(offset, bytes)) {
                    if is_inline {
                        self.walk_inline_value(func, ptr);
                    } else {
                        self.walk_inline_expression(func, ptr);
                    }

                    self.output.push_str(&format!(".field_{}", offset));
                    handled = true;
                }
            }
        }

        if !handled {
            // Fallback to the pointer-based logic for locals if we have no annotation
            if let ExpressionKind::LocalGet { id } = &self.module.expressions[ptr].kind {
                let local = &func.locals()[*id];
                if let Some(crate::ir::annotation::HighLevelType::Pointer) = local.h_type {
                    let name = self.get_local_name(func, *id, Some(ptr));
                    if offset == 0 {
                        self.output.push_str(&format!("*{}", name));
                    } else {
                        self.output.push_str(&format!("{}.field_{}", name, offset));
                    }
                    handled = true;
                }
            }
        }

        if !handled {
            self.output.push_str("*(");
            if is_inline {
                self.walk_inline_value(func, ptr);
            } else {
                self.walk_inline_expression(func, ptr);
            }
            if offset > 0 {
                self.output.push_str(&format!(" + {}", offset));
            }
            self.output.push_str(")");
        }

        if !is_inline {
            self.output.push_str("");
        }
    }

    fn print_store(
        &mut self,
        func: &Function,
        _expr_id: ExpressionId,
        ptr: ExpressionId,
        value: ExpressionId,
        offset: u32,
        bytes: u32,
        is_inline: bool,
    ) {
        let mut handled = false;

        // Try to handle as struct access via annotations
        if let Some(ann) = self.module.annotations.get(ptr) {
            if let Some(offsets) = &ann.struct_offsets {
                if offsets.contains(&(offset, bytes)) {
                    if is_inline {
                        self.walk_inline_value(func, ptr);
                    } else {
                        self.walk_inline_expression(func, ptr);
                    }
                    if offset == 0 {
                        self.output.push_str(".field_0");
                    } else {
                        self.output.push_str(&format!(".field_{}", offset));
                    }
                    self.output.push_str(" = ");
                    if is_inline {
                        self.walk_inline_value(func, value);
                    } else {
                        self.walk_inline_expression(func, value);
                    }
                    handled = true;
                }
            }
        }

        if !handled {
            // Fallback to the pointer-based logic for locals if we have no annotation
            if let ExpressionKind::LocalGet { id } = &self.module.expressions[ptr].kind {
                let local = &func.locals()[*id];
                if let Some(crate::ir::annotation::HighLevelType::Pointer) = local.h_type {
                    let name = self.get_local_name(func, *id, Some(ptr));
                    if offset == 0 {
                        self.output.push_str(&format!("*{} = ", name));
                    } else {
                        self.output
                            .push_str(&format!("{}.field_{} = ", name, offset));
                    }
                    if is_inline {
                        self.walk_inline_value(func, value);
                    } else {
                        self.walk_inline_expression(func, value);
                    }
                    handled = true;
                }
            }
        }

        if !handled {
            self.output.push_str("*(");
            if is_inline {
                self.walk_inline_value(func, ptr);
            } else {
                self.walk_inline_expression(func, ptr);
            }
            if offset != 0 {
                self.output.push_str(&format!(" + {}", offset));
            }
            self.output.push_str(") = ");
            if is_inline {
                self.walk_inline_value(func, value);
            } else {
                self.walk_inline_expression(func, value);
            }
        }
    }

    fn is_at_start(&self, parent_id: ExpressionId, target_id: ExpressionId) -> bool {
        if parent_id == target_id {
            return true;
        }
        let expr = &self.module.expressions[parent_id];
        match &expr.kind {
            ExpressionKind::Block { list, .. } => {
                if !list.is_empty() {
                    return self.is_at_start(list[0], target_id);
                }
            }
            ExpressionKind::If { if_true, .. } => {
                return self.is_at_start(*if_true, target_id);
            }
            ExpressionKind::Loop { body, .. } => {
                return self.is_at_start(*body, target_id);
            }
            _ => {}
        }
        false
    }

    fn walk_expression_ext_no_indent(
        &mut self,
        func: &Function,
        expr_id: ExpressionId,
        is_statement: bool,
    ) {
        if self.module.annotations.is_inlined(expr_id) {
            return;
        }

        let expr = &self.module.expressions[expr_id];
        let is_return = self.is_last_in_func(func, expr_id) && func.results != Type::NONE;

        // If this expression is in a return position and is printable as an inline
        // expression, print it as `return <expr>;` and avoid the generic fallback.
        if is_return {
            match &expr.kind {
                ExpressionKind::Const(_)
                | ExpressionKind::LocalGet { .. }
                | ExpressionKind::Binary { .. }
                | ExpressionKind::Unary { .. }
                | ExpressionKind::GlobalGet { .. }
                | ExpressionKind::Load { .. }
                | ExpressionKind::Select { .. }
                | ExpressionKind::TupleMake { .. }
                | ExpressionKind::RefFunc { .. }
                | ExpressionKind::LocalTee { .. }
                | ExpressionKind::CallIndirect { .. } => {
                    self.output.push_str("return ");
                    self.walk_inline_value(func, expr_id);
                    self.output.push_str(";\n");
                    return;
                }
                _ => {}
            }
        }

        self.walk_expression_internal(func, expr_id, is_statement, is_return);
    }

    fn walk_body(&mut self, func: &Function, target_id: ExpressionId) {
        if self.module.annotations.is_inlined(target_id) {
            return;
        }
        if let ExpressionKind::Block { list, .. } = &self.module.expressions[target_id].kind {
            self.walk_block_list(func, target_id, list, 0);
        } else {
            self.walk_expression_ext(func, target_id, true);
        }
    }

    fn walk_expression_ext(&mut self, func: &Function, expr_id: ExpressionId, is_statement: bool) {
        if self.module.annotations.is_inlined(expr_id) {
            return;
        }

        self.write_indent();
        self.walk_expression_ext_no_indent(func, expr_id, is_statement);
    }

    fn walk_expression_internal(
        &mut self,
        func: &Function,
        expr_id: ExpressionId,
        is_statement: bool,
        is_return: bool,
    ) {
        let expr = &self.module.expressions[expr_id];
        match &expr.kind {
            ExpressionKind::Block {
                name: block_name,
                list,
            } => {
                // If this is an anonymous block with one child,
                // and it's not targeted, we can strip the braces.
                // We also check get_if_info because that special-cases some blocks.
                if block_name.is_none()
                    && list.len() == 1
                    && !self.find_all_targets.contains(&expr_id)
                    && self.module.annotations.get_if_info(expr_id).is_none()
                {
                    self.walk_expression_internal(func, list[0], is_statement, is_return);
                    return;
                }

                let mut loop_nested = None;
                if list.len() == 1 {
                    if let ExpressionKind::Loop { .. } = &self.module.expressions[list[0]].kind {
                        loop_nested = Some(list[0]);
                    }
                }

                if let Some(loop_id) = loop_nested {
                    let loop_expr = &self.module.expressions[loop_id];
                    let (loop_name, loop_body) =
                        if let ExpressionKind::Loop { name, body } = &loop_expr.kind {
                            (name, body)
                        } else {
                            unreachable!()
                        };

                    self.emit_block_label_if_needed(func, expr_id, block_name);
                    if block_name.is_none() {
                        self.emit_block_label_if_needed(func, loop_id, loop_name);
                    }

                    self.output.push_str("loop {\n");

                    self.loop_stack.push(LoopContext {
                        break_target: Some(expr_id),
                        continue_target: Some(loop_id),
                    });
                    self.indent += 1;
                    self.walk_expression_ext(func, *loop_body, true);
                    self.indent -= 1;
                    self.loop_stack.pop();

                    self.write_indent();
                    self.output.push_str("}\n");
                    return;
                }

                let mut is_if = false;
                let mut wrapped_if = false;
                if let Some((condition, inverted)) = self.module.annotations.get_if_info(expr_id) {
                    is_if = true;
                    if self.find_all_targets.contains(&expr_id) || block_name.is_some() {
                        wrapped_if = true;
                        self.emit_block_label_if_needed(func, expr_id, block_name);
                        self.output.push_str("{\n");
                        self.indent += 1;
                        self.write_indent();
                    }
                    self.output.push_str("if ");
                    if inverted {
                        self.output.push_str("!");
                    }
                    self.render_boolean_condition(func, condition);
                    self.output.push_str(" ");
                } else {
                    self.emit_block_label_if_needed(func, expr_id, block_name);
                }

                self.output.push_str("{\n");
                self.indent += 1;

                let start_idx = if is_if { 1 } else { 0 };
                self.walk_block_list(func, expr_id, list, start_idx);

                self.indent -= 1;
                self.write_indent();
                self.output.push_str("}\n");

                if wrapped_if {
                    self.indent -= 1;
                    self.write_indent();
                    self.output.push_str("}\n");
                }
            }
            ExpressionKind::LocalSet { id, value } => {
                let name = self.get_local_name(func, *id, Some(expr_id));
                self.output.push_str(&format!("{} = ", name));
                self.walk_inline_expression(func, *value);
                if is_statement {
                    self.output.push_str(";\n");
                }
            }
            ExpressionKind::Call {
                target, operands, ..
            } => {
                if let Some(target_func) = self.module.functions.get(*target) {
                    self.output.push_str(&format!("{}(", target_func.name));
                    for (i, &op_id) in operands.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.walk_inline_expression(func, op_id);
                    }
                    self.output.push_str(")");
                    if is_statement && !is_return {
                        self.output.push_str(";\n");
                    } else {
                        self.output.push_str("\n");
                    }
                }
            }
            ExpressionKind::If {
                name,
                condition,
                if_true,
                if_false,
            } => {
                let has_label = self.emit_block_label_if_needed(func, expr_id, name);
                if has_label {
                    self.output.push_str("{ ");
                }

                self.output.push_str("if ");
                self.render_boolean_condition(func, *condition);
                self.output.push_str(" {\n");
                self.indent += 1;

                self.walk_body(func, *if_true);

                self.indent -= 1;
                self.write_indent();
                self.output.push_str("}");

                if let Some(f) = if_false {
                    // Omit printing `else` when the branch is an explicit NOP
                    match &self.module.expressions[*f].kind {
                        ExpressionKind::Nop => {}
                        _ => {
                            self.output.push_str(" else ");
                            if let ExpressionKind::If { .. } = &self.module.expressions[*f].kind {
                                // For nested if, use walk_expression_ext without indent?
                                // Actually, let's just handle it here to keep correct flow.
                                self.walk_expression_ext_no_indent(func, *f, is_statement);
                            } else {
                                self.output.push_str("{\n");
                                self.indent += 1;
                                self.walk_body(func, *f);
                                self.indent -= 1;
                                self.write_indent();
                                self.output.push_str("}");
                            }
                        }
                    }
                }

                if has_label {
                    self.output.push_str(" }");
                }
                self.output.push_str("\n");
            }
            ExpressionKind::Loop { name, body } => {
                self.emit_block_label_if_needed(func, expr_id, name);
                self.output.push_str("loop {\n");
                self.indent += 1;
                // A standalone loop's own break target is NOT itself in Wasm
                // (Wasm `loop` labels are for `continue`).
                // But in Rust, `continue` targets the loop itself.
                // We don't have a specific wrapping block here if this is a standalone
                // ExpressionKind::Loop, so we only push the continue target.
                self.loop_stack.push(LoopContext {
                    break_target: None, // Standalone loop has no implicit break target
                    continue_target: Some(expr_id),
                });
                self.walk_body(func, *body);
                self.loop_stack.pop();
                self.indent -= 1;
                self.write_indent();
                self.output.push_str("}\n");
            }
            ExpressionKind::Break {
                target,
                condition,
                value,
            } => {
                let mut idiomatic_keyword = None;
                if let Some(ctx) = self.loop_stack.last() {
                    if Some(*target) == ctx.break_target && value.is_none() {
                        idiomatic_keyword = Some("break");
                    } else if Some(*target) == ctx.continue_target && value.is_none() {
                        idiomatic_keyword = Some("continue");
                    }
                }

                if let Some(kw) = idiomatic_keyword {
                    if let Some(cond) = condition {
                        self.output.push_str("if ");
                        self.render_boolean_condition(func, *cond);
                        self.output.push_str(&format!(" {{ {} }}\n", kw));
                    } else {
                        self.output.push_str(&format!("{};\n", kw));
                    }
                    return;
                }

                let (target_label, is_loop) = match &self.module.expressions[*target].kind {
                    ExpressionKind::Block { name, .. } | ExpressionKind::If { name, .. } => {
                        let label = if let Some(n) = name {
                            Self::sanitize_label(n)
                        } else {
                            format!("L{}", target.index())
                        };
                        (label, false)
                    }
                    ExpressionKind::Loop { name, .. } => {
                        let label = if let Some(n) = name {
                            Self::sanitize_label(n)
                        } else {
                            format!("L{}", target.index())
                        };
                        (label, true)
                    }
                    _ => (format!("target_{}", target.index()), false),
                };

                let keyword = if is_loop { "continue" } else { "break" };

                if let Some(cond) = condition {
                    self.output.push_str("if ");
                    self.render_boolean_condition(func, *cond);
                    self.output.push_str(" {\n");
                    self.indent += 1;
                    self.write_indent();
                    if let Some(v) = value {
                        self.output.push_str("/* break with value: ");
                        self.walk_inline_expression(func, *v);
                        self.output.push_str(" */;\n");
                        self.write_indent();
                    }
                    self.output
                        .push_str(&format!("{} '{};\n", keyword, target_label));
                    self.indent -= 1;
                    self.write_indent();
                    self.output.push_str("}\n");
                } else {
                    if let Some(v) = value {
                        self.output.push_str("/* break with value: ");
                        self.walk_inline_expression(func, *v);
                        self.output.push_str(" */;\n");
                        self.write_indent();
                    }
                    self.output
                        .push_str(&format!("{} '{};\n", keyword, target_label));
                }
            }
            ExpressionKind::LocalTee { id, value } => {
                let name = self.get_local_name(func, *id, Some(expr_id));
                if is_statement {
                    self.output.push_str(&format!("{} = ", name));
                    self.walk_inline_expression(func, *value);
                    self.output.push_str(";\n");
                } else {
                    self.output.push_str(&format!("{{ {} = ", name));
                    self.walk_inline_expression(func, *value);
                    self.output.push_str(&format!("; {} }}", name));
                }
            }
            ExpressionKind::Nop => {
                self.output.push_str("/* nop */\n");
            }
            ExpressionKind::Unreachable => {
                self.output.push_str("panic!(\"unreachable\");\n");
            }
            ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            } => {
                if is_return {
                    self.output.push_str("return ");
                }
                self.output.push_str("if ");
                self.walk_inline_expression(func, *condition);
                self.output.push_str(" { ");
                self.walk_inline_expression(func, *if_true);
                self.output.push_str(" } else { ");
                self.walk_inline_expression(func, *if_false);
                self.output.push_str(" }");
                if is_statement {
                    self.output.push_str(";\n");
                } else {
                    self.output.push_str("\n");
                }
            }
            ExpressionKind::Store {
                ptr,
                value,
                offset,
                bytes,
                ..
            } => {
                self.print_store(func, expr_id, *ptr, *value, *offset, *bytes, false);
                self.output.push_str(";\n");
            }
            ExpressionKind::Load { .. }
            | ExpressionKind::GlobalGet { .. }
            | ExpressionKind::LocalGet { .. }
            | ExpressionKind::Binary { .. }
            | ExpressionKind::Unary { .. }
            | ExpressionKind::Const(_) => {
                if is_return {
                    self.output.push_str("return ");
                }
                self.walk_inline_value(func, expr_id);
                if is_statement {
                    self.output.push_str(";\n");
                } else {
                    self.output.push_str("\n");
                }
            }
            ExpressionKind::GlobalSet { id, value } => {
                if let Some(g) = self.module.globals.get(id.cast()) {
                    self.output.push_str(&format!("{} = ", g.name));
                } else {
                    self.output.push_str(&format!("global_{} = ", id.index()));
                }
                self.walk_inline_expression(func, *value);
                self.output.push_str(";\n");
            }
            ExpressionKind::CallIndirect {
                target, operands, ..
            } => {
                if is_return {
                    self.output.push_str("return ");
                }
                self.walk_inline_expression(func, *target);
                self.output.push_str("(");
                for (i, op) in operands.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.walk_inline_expression(func, *op);
                }
                self.output.push_str(")");
                if is_statement {
                    self.output.push_str(";\n");
                } else {
                    self.output.push_str("\n");
                }
            }
            ExpressionKind::Try {
                body, catch_bodies, ..
            } => {
                self.output.push_str("/* try */ {\n");
                self.indent += 1;
                self.walk_body(func, *body);
                self.indent -= 1;
                self.write_indent();
                self.output.push_str("}");
                for &cb in catch_bodies {
                    self.output.push_str(" /* catch */ {\n");
                    self.indent += 1;
                    self.walk_body(func, cb);
                    self.indent -= 1;
                    self.write_indent();
                    self.output.push_str("}");
                }
                self.output.push_str("\n");
            }
            ExpressionKind::Return { value } => {
                self.output.push_str("return");
                if let Some(v) = value {
                    self.output.push_str(" ");
                    self.walk_inline_expression(func, *v);
                }
                self.output.push_str(";\n");
            }
            ExpressionKind::Switch {
                targets,
                default,
                condition,
                value,
            } => {
                self.output.push_str("match ");
                self.walk_inline_expression(func, *condition);
                self.output.push_str(" {\n");
                let old_indent = self.indent;
                self.indent += 1;

                // Group cases by target
                let mut map = std::collections::BTreeMap::new();
                for (i, &t) in targets.iter().enumerate() {
                    map.entry(t).or_insert_with(Vec::new).push(i);
                }

                for (target_id, indices) in map {
                    self.write_indent();
                    for (i, &idx) in indices.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(" | ");
                        }
                        self.output.push_str(&format!("{}", idx));
                    }
                    self.output.push_str(" => ");
                    self.print_switch_target(func, target_id, *value);
                }

                self.write_indent();
                self.output.push_str("_ => ");
                self.print_switch_target(func, *default, *value);

                self.indent = old_indent;
                self.write_indent();
                self.output.push_str("}\n");
            }
            ExpressionKind::TableSet {
                table,
                index,
                value,
            } => {
                self.walk_inline_expression(func, *index);
                self.output
                    .push_str(&format!(" /* table {} */ = ", table.index()));
                self.walk_inline_expression(func, *value);
                self.output.push_str(";\n");
            }
            ExpressionKind::TableGrow {
                table,
                value,
                delta: _,
            } => {
                self.output
                    .push_str(&format!("/* table.grow {} */ ", table.index()));
                self.walk_inline_expression(func, *value);
                self.output.push_str(";");
                self.output.push_str("\n");
            }
            ExpressionKind::TableFill {
                table,
                dest,
                value,
                size,
            } => {
                self.output
                    .push_str(&format!("/* table.fill {} */ ", table.index()));
                self.walk_inline_expression(func, *dest);
                self.output.push_str(" = ");
                self.walk_inline_expression(func, *value);
                self.output.push_str("; /* size: ");
                self.walk_inline_expression(func, *size);
                self.output.push_str(" */\n");
            }
            ExpressionKind::TableCopy { .. } => {
                self.output.push_str("/* table.copy */\n");
            }
            ExpressionKind::TableInit { .. } => {
                self.output.push_str("/* table.init */\n");
            }
            ExpressionKind::ElemDrop { segment } => {
                self.output
                    .push_str(&format!("/* elem.drop {} */\n", segment.index()));
            }
            ExpressionKind::MemoryCopy {
                dest, src, size, ..
            } => {
                self.output.push_str("memory_copy(");
                self.walk_inline_value(func, *dest);
                self.output.push_str(", ");
                self.walk_inline_value(func, *src);
                self.output.push_str(", ");
                self.walk_inline_value(func, *size);
                self.output.push_str(")");
                if is_statement {
                    self.output.push_str(";\n");
                }
            }
            ExpressionKind::MemoryFill {
                dest, value, size, ..
            } => {
                self.output.push_str("memory_fill(");
                self.walk_inline_value(func, *dest);
                self.output.push_str(", ");
                self.walk_inline_value(func, *value);
                self.output.push_str(", ");
                self.walk_inline_value(func, *size);
                self.output.push_str(")");
                if is_statement {
                    self.output.push_str(";\n");
                }
            }
            ExpressionKind::DataDrop { segment } => {
                self.output
                    .push_str(&format!("/* data.drop {} */\n", segment.index()));
            }
            ExpressionKind::StructSet {
                type_: _,
                ptr,
                index,
                value,
            } => {
                self.walk_inline_expression(func, *ptr);
                self.output.push_str(&format!(".field{} = ", index));
                self.walk_inline_expression(func, *value);
                self.output.push_str(";\n");
            }
            ExpressionKind::ArraySet {
                type_: _,
                ptr,
                index,
                value,
            } => {
                self.walk_inline_expression(func, *ptr);
                self.output.push_str("[");
                self.walk_inline_expression(func, *index);
                self.output.push_str("] = ");
                self.walk_inline_expression(func, *value);
                self.output.push_str(";\n");
            }
            ExpressionKind::Drop { value } => {
                self.walk_inline_expression(func, *value);
                self.output.push_str(";\n");
            }
            ExpressionKind::Rethrow { target } => {
                self.output.push_str(&format!("/* rethrow {} */\n", target));
            }
            ExpressionKind::RefTest {
                op: _,
                value,
                type_: _,
            } => {
                self.output.push_str("/* ref.test */ ");
                self.walk_inline_expression(func, *value);
                self.output.push_str(";\n");
            }
            ExpressionKind::RefCast {
                op: _,
                value,
                type_: _,
            } => {
                self.output.push_str("/* ref.cast */ ");
                self.walk_inline_expression(func, *value);
                self.output.push_str(";\n");
            }
            ExpressionKind::BrOn {
                target,
                op: _,
                value: _,
                type_: _,
            } => {
                self.output.push_str(&format!("/* br_on {:?} */\n", target));
            }
            ExpressionKind::Throw { tag, operands: _ } => {
                self.output.push_str(&format!("/* throw {} */\n", tag));
            }
            _ => {
                if is_return {
                    self.output.push_str("return ");
                }
                self.output
                    .push_str(&format!("/* unsupported expr: {:?} */", expr.kind));
                self.output.push_str(";\n");
            }
        }
    }

    fn walk_inline_value(&mut self, func: &Function, expr_id: ExpressionId) {
        // Like `walk_inline_expression`, but unwraps `LocalTee` and recurses into
        // child expressions using `walk_inline_value` so temporaries introduced
        // by the lifter are not printed as `(<tmp> = <val>)`.
        if let Some(inlined_id) = self.module.annotations.get_inlined_value(expr_id) {
            self.walk_inline_value(func, inlined_id);
            return;
        }

        if let Some(lit) = self.module.annotations.get_constant_global(expr_id) {
            self.output.push_str(&format!("{} /* global */", lit));
            return;
        }

        let expr = &self.module.expressions[expr_id];
        match &expr.kind {
            ExpressionKind::Const(lit) => {
                self.output.push_str(&format!("{}", lit));
            }
            ExpressionKind::LocalGet { id } => {
                let name = self.get_local_name(func, *id, Some(expr_id));
                self.output.push_str(&name);
            }
            ExpressionKind::Binary { op, left, right } => {
                // For commutative ops like add/mul, prefer a canonical ordering
                // when both operands are simple locals so output is stable.
                let commutative = matches!(
                    op,
                    crate::ir::ops::BinaryOp::AddInt32
                        | crate::ir::ops::BinaryOp::AddInt64
                        | crate::ir::ops::BinaryOp::MulInt32
                        | crate::ir::ops::BinaryOp::MulInt64
                );

                // Helper to extract a local id index if this expression is a direct LocalGet
                // or a LocalTee whose value is a LocalGet
                let extract_local = |printer: &RustPrinter, id: ExpressionId| -> Option<usize> {
                    if let ExpressionKind::LocalGet { id: lid } =
                        &printer.module.expressions[id].kind
                    {
                        return Some(lid.index().try_into().unwrap());
                    }
                    if let ExpressionKind::LocalTee { value, .. } =
                        &printer.module.expressions[id].kind
                    {
                        if let ExpressionKind::LocalGet { id: lid } =
                            &printer.module.expressions[*value].kind
                        {
                            return Some(lid.index().try_into().unwrap());
                        }
                    }
                    None
                };

                let left_local_idx = extract_local(self, *left);
                let right_local_idx = extract_local(self, *right);

                // TODO: create a data structure for operator strings, and look up from there instead of this massive verbose match
                let op_str = match op {
                    crate::ir::ops::BinaryOp::AddInt32 | crate::ir::ops::BinaryOp::AddInt64 => "+",
                    crate::ir::ops::BinaryOp::SubInt32 | crate::ir::ops::BinaryOp::SubInt64 => "-",
                    crate::ir::ops::BinaryOp::MulInt32 | crate::ir::ops::BinaryOp::MulInt64 => "*",
                    crate::ir::ops::BinaryOp::DivSInt32 | crate::ir::ops::BinaryOp::DivSInt64 => {
                        "/"
                    }
                    crate::ir::ops::BinaryOp::DivUInt32 | crate::ir::ops::BinaryOp::DivUInt64 => {
                        "/"
                    }
                    crate::ir::ops::BinaryOp::EqInt32 | crate::ir::ops::BinaryOp::EqInt64 => "==",
                    crate::ir::ops::BinaryOp::NeInt32 | crate::ir::ops::BinaryOp::NeInt64 => "!=",
                    crate::ir::ops::BinaryOp::LtSInt32 | crate::ir::ops::BinaryOp::LtSInt64 => "<",
                    crate::ir::ops::BinaryOp::LtUInt32 | crate::ir::ops::BinaryOp::LtUInt64 => "<",
                    crate::ir::ops::BinaryOp::LeSInt32 | crate::ir::ops::BinaryOp::LeSInt64 => "<=",
                    crate::ir::ops::BinaryOp::LeUInt32 | crate::ir::ops::BinaryOp::LeUInt64 => "<=",
                    crate::ir::ops::BinaryOp::GtSInt32 | crate::ir::ops::BinaryOp::GtSInt64 => ">",
                    crate::ir::ops::BinaryOp::GtUInt32 | crate::ir::ops::BinaryOp::GtUInt64 => ">",
                    crate::ir::ops::BinaryOp::GeSInt32 | crate::ir::ops::BinaryOp::GeSInt64 => ">=",
                    crate::ir::ops::BinaryOp::GeUInt32 | crate::ir::ops::BinaryOp::GeUInt64 => ">=",
                    crate::ir::ops::BinaryOp::AndInt32 | crate::ir::ops::BinaryOp::AndInt64 => "&",
                    crate::ir::ops::BinaryOp::OrInt32 | crate::ir::ops::BinaryOp::OrInt64 => "|",
                    crate::ir::ops::BinaryOp::XorInt32 | crate::ir::ops::BinaryOp::XorInt64 => "^",
                    crate::ir::ops::BinaryOp::ShlInt32 | crate::ir::ops::BinaryOp::ShlInt64 => "<<",
                    crate::ir::ops::BinaryOp::ShrSInt32 | crate::ir::ops::BinaryOp::ShrSInt64 => {
                        ">>"
                    }
                    crate::ir::ops::BinaryOp::ShrUInt32 | crate::ir::ops::BinaryOp::ShrUInt64 => {
                        ">>"
                    }
                    _ => "?",
                };

                self.output.push_str("(");
                if commutative {
                    if let (Some(li), Some(ri)) = (left_local_idx, right_local_idx) {
                        if li <= ri {
                            self.walk_inline_value(func, *left);
                            self.output.push_str(&format!(" {} ", op_str));
                            self.walk_inline_value(func, *right);
                        } else {
                            self.walk_inline_value(func, *right);
                            self.output.push_str(&format!(" {} ", op_str));
                            self.walk_inline_value(func, *left);
                        }
                    } else {
                        self.walk_inline_value(func, *left);
                        self.output.push_str(&format!(" {} ", op_str));
                        self.walk_inline_value(func, *right);
                    }
                } else {
                    self.walk_inline_value(func, *left);
                    self.output.push_str(&format!(" {} ", op_str));
                    self.walk_inline_value(func, *right);
                }
                self.output.push_str(")");
            }
            ExpressionKind::Unary { op, value } => {
                let op_str = match op {
                    crate::ir::ops::UnaryOp::ClzInt32 | crate::ir::ops::UnaryOp::ClzInt64 => "clz",
                    crate::ir::ops::UnaryOp::CtzInt32 | crate::ir::ops::UnaryOp::CtzInt64 => "ctz",
                    crate::ir::ops::UnaryOp::PopcntInt32 | crate::ir::ops::UnaryOp::PopcntInt64 => {
                        "popcnt"
                    }
                    _ => "unary",
                };
                self.output.push_str(&format!("{}(", op_str));
                self.walk_inline_value(func, *value);
                self.output.push_str(")");
            }
            ExpressionKind::LocalTee { id, value } => {
                let name = self.get_local_name(func, *id, Some(expr_id));
                self.output.push_str("{ ");
                self.output.push_str(&format!("{} = ", name));
                self.walk_inline_value(func, *value);
                self.output.push_str(&format!("; {} }}", name));
            }
            ExpressionKind::GlobalGet { id } => {
                if let Some(g) = self.module.globals.get(id.cast()) {
                    self.output.push_str(&g.name);
                } else {
                    self.output.push_str(&format!("global_{}", id.index()));
                }
            }
            ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            } => {
                self.output.push_str("if ");
                self.walk_inline_value(func, *condition);
                self.output.push_str(" { ");
                self.walk_inline_value(func, *if_true);
                self.output.push_str(" } else { ");
                self.walk_inline_value(func, *if_false);
                self.output.push_str(" }");
            }
            ExpressionKind::Load {
                ptr, offset, bytes, ..
            } => {
                self.print_load(func, expr_id, *ptr, *offset, *bytes, true);
            }
            ExpressionKind::Store {
                ptr,
                value,
                offset,
                bytes,
                ..
            } => {
                self.print_store(func, expr_id, *ptr, *value, *offset, *bytes, true);
            }
            ExpressionKind::Call {
                target: f_id,
                operands,
                ..
            } => {
                if let Some(target) = self.module.functions.get(*f_id) {
                    self.output.push_str(&target.name);
                } else {
                    self.output.push_str(&format!("func_{}", f_id.index()));
                }
                self.output.push_str("(");
                for (i, id) in operands.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.walk_inline_value(func, *id);
                }
                self.output.push_str(")");
            }
            ExpressionKind::RefFunc { func } => {
                if let Some(f) = self.module.functions.get(*func) {
                    self.output.push_str(&format!("&{}", f.name));
                } else {
                    self.output.push_str("&func");
                }
            }
            ExpressionKind::TableGet { table, index } => {
                self.output.push_str(&format!("table_{}[", table.index()));
                self.walk_inline_value(func, *index);
                self.output.push_str("]");
            }
            ExpressionKind::TupleExtract { tuple, index } => {
                self.output.push_str("/* tuple.extract */ ");
                self.walk_inline_value(func, *tuple);
                self.output.push_str(&format!(".{}", index));
            }
            ExpressionKind::RefNull { type_: _ } => {
                self.output.push_str("None /* ref.null */");
            }
            ExpressionKind::RefIsNull { value } => {
                self.output.push_str("(/* ref.is_null */ ");
                self.walk_inline_value(func, *value);
                self.output.push_str(")");
            }
            ExpressionKind::RefEq { left, right } => {
                self.walk_inline_value(func, *left);
                self.output.push_str(" == ");
                self.walk_inline_value(func, *right);
            }
            ExpressionKind::RefAs { op: _, value } => {
                self.output.push_str("/* ref.as */ ");
                self.walk_inline_value(func, *value);
            }
            ExpressionKind::TableSize { table } => {
                self.output
                    .push_str(&format!("table_{}.len()", table.index()));
            }
            _ => {
                // Fallback to the generic inline printer for anything we didn't
                // explicitly handle here.
                self.walk_inline_expression(func, expr_id);
            }
        }
    }

    fn walk_inline_expression(&mut self, func: &Function, expr_id: ExpressionId) {
        if let Some(inlined_id) = self.module.annotations.get_inlined_value(expr_id) {
            self.walk_inline_expression(func, inlined_id);
            return;
        }

        if let Some(lit) = self.module.annotations.get_constant_global(expr_id) {
            self.output.push_str(&format!("{} /* global */", lit));
            return;
        }

        let expr = &self.module.expressions[expr_id];
        match &expr.kind {
            ExpressionKind::Const(lit) => {
                self.output.push_str(&format!("{}", lit));
            }
            ExpressionKind::LocalGet { id } => {
                let name = self.get_local_name(func, *id, Some(expr_id));
                self.output.push_str(&name);
            }
            ExpressionKind::Binary { op, left, right } => {
                self.output.push_str("(");
                self.walk_inline_expression(func, *left);
                let op_str = match op {
                    crate::ir::ops::BinaryOp::AddInt32 | crate::ir::ops::BinaryOp::AddInt64 => "+",
                    crate::ir::ops::BinaryOp::SubInt32 | crate::ir::ops::BinaryOp::SubInt64 => "-",
                    crate::ir::ops::BinaryOp::MulInt32 | crate::ir::ops::BinaryOp::MulInt64 => "*",
                    crate::ir::ops::BinaryOp::DivSInt32 | crate::ir::ops::BinaryOp::DivSInt64 => {
                        "/"
                    }
                    crate::ir::ops::BinaryOp::DivUInt32 | crate::ir::ops::BinaryOp::DivUInt64 => {
                        "/"
                    }
                    crate::ir::ops::BinaryOp::RemSInt32 | crate::ir::ops::BinaryOp::RemSInt64 => {
                        "%"
                    }
                    crate::ir::ops::BinaryOp::RemUInt32 | crate::ir::ops::BinaryOp::RemUInt64 => {
                        "%"
                    }
                    crate::ir::ops::BinaryOp::EqInt32 | crate::ir::ops::BinaryOp::EqInt64 => "==",
                    crate::ir::ops::BinaryOp::EqFloat32 | crate::ir::ops::BinaryOp::EqFloat64 => {
                        "=="
                    }
                    crate::ir::ops::BinaryOp::NeInt32 | crate::ir::ops::BinaryOp::NeInt64 => "!=",
                    crate::ir::ops::BinaryOp::NeFloat32 | crate::ir::ops::BinaryOp::NeFloat64 => {
                        "!="
                    }
                    crate::ir::ops::BinaryOp::LtSInt32 | crate::ir::ops::BinaryOp::LtSInt64 => "<",
                    crate::ir::ops::BinaryOp::LtUInt32 | crate::ir::ops::BinaryOp::LtUInt64 => "<",
                    crate::ir::ops::BinaryOp::LtFloat32 | crate::ir::ops::BinaryOp::LtFloat64 => {
                        "<"
                    }
                    crate::ir::ops::BinaryOp::LeSInt32 | crate::ir::ops::BinaryOp::LeSInt64 => "<=",
                    crate::ir::ops::BinaryOp::LeUInt32 | crate::ir::ops::BinaryOp::LeUInt64 => "<=",
                    crate::ir::ops::BinaryOp::LeFloat32 | crate::ir::ops::BinaryOp::LeFloat64 => {
                        "<="
                    }
                    crate::ir::ops::BinaryOp::GtSInt32 | crate::ir::ops::BinaryOp::GtSInt64 => ">",
                    crate::ir::ops::BinaryOp::GtUInt32 | crate::ir::ops::BinaryOp::GtUInt64 => ">",
                    crate::ir::ops::BinaryOp::GtFloat32 | crate::ir::ops::BinaryOp::GtFloat64 => {
                        ">"
                    }
                    crate::ir::ops::BinaryOp::GeSInt32 | crate::ir::ops::BinaryOp::GeSInt64 => ">=",
                    crate::ir::ops::BinaryOp::GeUInt32 | crate::ir::ops::BinaryOp::GeUInt64 => ">=",
                    crate::ir::ops::BinaryOp::GeFloat32 | crate::ir::ops::BinaryOp::GeFloat64 => {
                        ">="
                    }
                    crate::ir::ops::BinaryOp::AndInt32 | crate::ir::ops::BinaryOp::AndInt64 => "&",
                    crate::ir::ops::BinaryOp::OrInt32 | crate::ir::ops::BinaryOp::OrInt64 => "|",
                    crate::ir::ops::BinaryOp::XorInt32 | crate::ir::ops::BinaryOp::XorInt64 => "^",
                    crate::ir::ops::BinaryOp::ShlInt32 | crate::ir::ops::BinaryOp::ShlInt64 => "<<",
                    crate::ir::ops::BinaryOp::ShrSInt32 | crate::ir::ops::BinaryOp::ShrSInt64 => {
                        ">>"
                    }
                    crate::ir::ops::BinaryOp::ShrUInt32 | crate::ir::ops::BinaryOp::ShrUInt64 => {
                        ">>"
                    }
                    _ => "?",
                };
                self.output.push_str(&format!(" {} ", op_str));
                self.walk_inline_expression(func, *right);
                self.output.push_str(")");
            }
            ExpressionKind::Unary { op, value } => {
                let op_str = match op {
                    crate::ir::ops::UnaryOp::ClzInt32 | crate::ir::ops::UnaryOp::ClzInt64 => "clz",
                    crate::ir::ops::UnaryOp::CtzInt32 | crate::ir::ops::UnaryOp::CtzInt64 => "ctz",
                    crate::ir::ops::UnaryOp::PopcntInt32 | crate::ir::ops::UnaryOp::PopcntInt64 => {
                        "popcnt"
                    }
                    crate::ir::ops::UnaryOp::EqZInt32 | crate::ir::ops::UnaryOp::EqZInt64 => "!",
                    _ => "unary",
                };
                if op_str == "!" {
                    self.output.push_str("!");
                } else {
                    self.output.push_str(&format!("{}(", op_str));
                }
                self.output.push_str("(");
                self.walk_inline_expression(func, *value);
                self.output.push_str(")");
                if op_str != "!" {
                    self.output.push_str(")");
                }
            }
            ExpressionKind::LocalTee { id, value } => {
                let name = self.get_local_name(func, *id, Some(expr_id));
                self.output.push_str("{ ");
                self.output.push_str(&name);
                self.output.push_str(" = ");
                self.walk_inline_expression(func, *value);
                self.output.push_str(&format!("; {} }}", name));
            }
            ExpressionKind::GlobalGet { id } => {
                if let Some(g) = self.module.globals.get(id.cast()) {
                    self.output.push_str(&g.name);
                } else {
                    self.output.push_str(&format!("global_{}", id.index()));
                }
            }
            ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            } => {
                self.output.push_str("if ");
                self.walk_inline_expression(func, *condition);
                self.output.push_str(" { ");
                self.walk_inline_expression(func, *if_true);
                self.output.push_str(" } else { ");
                self.walk_inline_expression(func, *if_false);
                self.output.push_str(" }");
            }
            ExpressionKind::Load {
                ptr, offset, bytes, ..
            } => {
                self.print_load(func, expr_id, *ptr, *offset, *bytes, false);
            }
            ExpressionKind::Store {
                ptr,
                value,
                offset,
                bytes,
                ..
            } => {
                self.print_store(func, expr_id, *ptr, *value, *offset, *bytes, false);
            }
            ExpressionKind::CallIndirect {
                target, operands, ..
            } => {
                self.walk_inline_expression(func, *target);
                self.output.push_str("(");
                for (i, op) in operands.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.walk_inline_expression(func, *op);
                }
                self.output.push_str(")");
            }
            ExpressionKind::TupleMake { operands } => {
                self.output.push_str("(");
                for (i, id) in operands.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.walk_inline_expression(func, *id);
                }
                self.output.push_str(")");
            }
            ExpressionKind::RefFunc { func } => {
                if let Some(f) = self.module.functions.get(*func) {
                    self.output.push_str(&format!("&{}", f.name));
                } else {
                    self.output.push_str("&func");
                }
            }
            ExpressionKind::StructGet {
                type_: _,
                ptr,
                index,
                signed: _,
            } => {
                self.walk_inline_expression(func, *ptr);
                self.output.push_str(&format!(".field{}", index));
            }
            ExpressionKind::ArrayGet {
                type_: _,
                ptr,
                index,
                signed: _,
            } => {
                self.walk_inline_expression(func, *ptr);
                self.output.push_str("[");
                self.walk_inline_expression(func, *index);
                self.output.push_str("]");
            }
            ExpressionKind::ArrayLen { ptr } => {
                self.walk_inline_expression(func, *ptr);
                self.output.push_str(".len()");
            }
            ExpressionKind::I31Get { i31, signed: _ } => {
                self.output.push_str("i31_get(");
                self.walk_inline_expression(func, *i31);
                self.output.push_str(")");
            }
            ExpressionKind::Pop { type_: _ } => {
                self.output.push_str("/* pop */");
            }
            _ => {
                self.output.push_str(&format!("/* {:?} */", expr.kind));
            }
        }
    }

    fn print_switch_target(
        &mut self,
        func: &Function,
        target: ExpressionId,
        value: Option<ExpressionId>,
    ) {
        let mut idiomatic_keyword = None;
        if let Some(ctx) = self.loop_stack.last() {
            if Some(target) == ctx.break_target && value.is_none() {
                idiomatic_keyword = Some("break");
            } else if Some(target) == ctx.continue_target && value.is_none() {
                idiomatic_keyword = Some("continue");
            }
        }

        if let Some(kw) = idiomatic_keyword {
            self.output.push_str(&format!(" {};\n", kw));
            return;
        }

        let (target_name, is_loop) = if let ExpressionKind::Block { name: Some(n), .. }
        | ExpressionKind::If { name: Some(n), .. } =
            &self.module.expressions[target].kind
        {
            (Some(Self::sanitize_label(n)), false)
        } else if let ExpressionKind::Loop { name: Some(n), .. } =
            &self.module.expressions[target].kind
        {
            (Some(Self::sanitize_label(n)), true)
        } else {
            (None, false)
        };

        let keyword = if is_loop { "continue" } else { "break" };

        if let Some(v) = value {
            self.output.push_str(&format!(" {} ", keyword));
            if let Some(n) = target_name {
                self.output.push_str(&format!("'{}: ", n));
            }
            self.walk_inline_expression(func, v);
            self.output.push_str(";\n");
        } else if let Some(n) = target_name {
            self.output.push_str(&format!(" {} '{};\n", keyword, n));
        } else {
            self.output.push_str(&format!(" {};\n", keyword));
        }
    }
}
