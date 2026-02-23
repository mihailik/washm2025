use crate::ir::expression::{ExpressionId, ExpressionKind};
use crate::ir::module::{Function, FunctionKind, Module};
use crate::ir::Type;

/// The RustPrinter converts the annotated IR into human-readable Rust code.
pub struct RustPrinter<'m> {
    pub module: &'m Module,
    output: String,
    indent: usize,
}

impl<'m> RustPrinter<'m> {
    pub fn new(module: &'m Module) -> Self {
        Self {
            module,
            output: String::new(),
            indent: 0,
        }
    }

    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
    }

    fn sanitize_label(name: &str) -> String {
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
            out = format!("label_{}", out);
        }
        out
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
            if *body == expr_id {
                return true;
            }
            // Check if it's the last expression in the top-level block
            match &self.module.expressions[*body].kind {
                ExpressionKind::Block { list, .. } => list.last() == Some(&expr_id),
                _ => false,
            }
        } else {
            false
        }
    }

    pub fn print(&mut self) -> String {
        self.output.clear();
        self.output
            .push_str("// Decompiled to Rust from WebAssembly\n\n");

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

    fn print_function(&mut self, func: &Function) {
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
            FunctionKind::Local { body, var_ids, .. } => {
                self.output.push_str(" {\n");
                self.indent += 1;

                // Declare locals
                for &id in var_ids {
                    let name = self.get_local_name(func, id, None);
                    if let FunctionKind::Local { locals, .. } = &func.kind {
                        let ty = locals[id].type_;
                        self.write_indent();
                        self.output.push_str(&format!(
                            "let mut {}: {} = 0;\n",
                            name,
                            self.type_name(ty)
                        ));
                    }
                }

                self.walk_expression_ext(func, *body, func.results != Type::NONE);

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

    fn walk_expression_ext(&mut self, func: &Function, expr_id: ExpressionId, is_statement: bool) {
        if self.module.annotations.is_inlined(expr_id) {
            return;
        }

        let expr = &self.module.expressions[expr_id];
        let is_return =
            is_statement && self.is_last_in_func(func, expr_id) && func.results != Type::NONE;

        self.write_indent();

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

        match &expr.kind {
            ExpressionKind::Block { name, list } => {
                let mut is_if = false;
                if let Some((condition, inverted)) = self.module.annotations.get_if_info(expr_id) {
                    is_if = true;
                    self.output.push_str("if ");
                    if inverted {
                        self.output.push_str("!");
                    }
                    // Prefer printing boolean constants as `true`/`false` when used as conditions
                    match &self.module.expressions[condition].kind {
                        ExpressionKind::Const(lit) => {
                            use crate::ir::Literal;
                            match lit {
                                Literal::I32(v) => {
                                    self.output.push_str(if *v != 0 { "true" } else { "false" })
                                }
                                Literal::I64(v) => {
                                    self.output.push_str(if *v != 0 { "true" } else { "false" })
                                }
                                _ => self.walk_inline_expression(func, condition),
                            }
                        }
                        _ => {
                            self.walk_inline_expression(func, condition);
                        }
                    }
                    self.output.push_str(" ");
                } else if let Some(n) = name {
                    let safe = Self::sanitize_label(n);
                    self.output.push_str(&format!("'{}: ", safe));
                }

                self.output.push_str("{\n");
                self.indent += 1;

                let start_idx = if is_if { 1 } else { 0 };
                let block_has_value = self.module.expressions[expr_id].type_ != Type::NONE
                    && self.module.expressions[expr_id].type_ != Type::UNREACHABLE;
                for (i, &child_id) in list.iter().enumerate().skip(start_idx) {
                    let child_is_last = i == list.len() - 1;
                    // A child is a statement when it's not the final value-producing
                    // expression of the block (which should be printed inline without
                    // a trailing semicolon). Otherwise it's a statement and should be
                    // printed with a semicolon/newline.
                    let child_is_statement = !(child_is_last && block_has_value);
                    self.walk_expression_ext(func, child_id, child_is_statement);
                }

                self.indent -= 1;
                self.write_indent();
                self.output.push_str("}\n");
            }
            ExpressionKind::LocalSet { id, value } => {
                let name = self.get_local_name(func, *id, Some(expr_id));
                self.output.push_str(&format!("{} = ", name));
                self.walk_inline_expression(func, *value);
                self.output.push_str(";\n");
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
                    if !is_return {
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
                if let Some(n) = name {
                    let safe = Self::sanitize_label(n);
                    self.output.push_str(&format!("'{}: ", safe));
                }
                self.output.push_str("if ");
                self.walk_inline_expression(func, *condition);
                self.output.push_str(" ");

                match &self.module.expressions[*if_true].kind {
                    ExpressionKind::Block { list, .. } => {
                        self.output.push_str("{\n");
                        self.indent += 1;
                        for &child in list {
                            self.walk_expression_ext(func, child, true);
                        }
                        self.indent -= 1;
                        self.write_indent();
                        self.output.push_str("}");
                    }
                    _ => {
                        self.output.push_str("{\n");
                        self.indent += 1;
                        self.walk_expression_ext(func, *if_true, true);
                        self.indent -= 1;
                        self.write_indent();
                        self.output.push_str("}");
                    }
                }

                if let Some(f) = if_false {
                    // Omit printing `else` when the branch is an explicit NOP
                    match &self.module.expressions[*f].kind {
                        ExpressionKind::Nop => {}
                        _ => {
                            self.output.push_str(" else ");
                            match &self.module.expressions[*f].kind {
                                ExpressionKind::Block { list, .. } => {
                                    self.output.push_str("{\n");
                                    self.indent += 1;
                                    for &child in list {
                                        self.walk_expression_ext(func, child, true);
                                    }
                                    self.indent -= 1;
                                    self.write_indent();
                                    self.output.push_str("}");
                                }
                                _ => {
                                    self.output.push_str("{\n");
                                    self.indent += 1;
                                    self.walk_expression_ext(func, *f, true);
                                    self.indent -= 1;
                                    self.write_indent();
                                    self.output.push_str("}");
                                }
                            }
                        }
                    }
                }
                self.output.push_str("\n");
            }
            ExpressionKind::Loop { name, body } => {
                if let Some(n) = name {
                    let safe = Self::sanitize_label(n);
                    self.output.push_str(&format!("'{}: ", safe));
                }
                self.output.push_str("loop {\n");
                self.indent += 1;
                self.walk_expression_ext(func, *body, false);
                self.indent -= 1;
                self.write_indent();
                self.output.push_str("}\n");
            }
            ExpressionKind::Break {
                target,
                condition,
                value,
            } => {
                let (target_name, is_loop) =
                    if let ExpressionKind::Block { name: Some(n), .. }
                    | ExpressionKind::If { name: Some(n), .. } =
                        &self.module.expressions[*target].kind
                    {
                        (Some(Self::sanitize_label(n)), false)
                    } else if let ExpressionKind::Loop { name: Some(n), .. } =
                        &self.module.expressions[*target].kind
                    {
                        (Some(Self::sanitize_label(n)), true)
                    } else {
                        (None, false)
                    };

                let keyword = if is_loop { "continue" } else { "break" };

                if let Some(cond) = condition {
                    self.output.push_str("if ");
                    self.walk_inline_expression(func, *cond);
                    self.output.push_str(" {\n");
                    self.indent += 1;
                    self.write_indent();
                    if let Some(v) = value {
                        self.output.push_str("/* break with value: ");
                        self.walk_inline_expression(func, *v);
                        self.output.push_str(" */;\n");
                        self.write_indent();
                    }
                    if let Some(n) = target_name {
                        self.output.push_str(&format!("{} '{};\n", keyword, n));
                    } else {
                        self.output.push_str(&format!("{};\n", keyword));
                    }
                    self.indent -= 1;
                    self.write_indent();
                    self.output.push_str("}\n");
                } else {
                    if let Some(v) = value {
                        self.output.push_str("/* break with value: ");
                        self.walk_inline_expression(func, *v);
                        self.output.push_str(" */;\n");
                    }
                    if let Some(n) = target_name {
                        self.output.push_str(&format!("{} '{};\n", keyword, n));
                    } else {
                        self.output.push_str(&format!("{};\n", keyword));
                    }
                }
            }
            ExpressionKind::LocalTee { id, value } => {
                let name = self.get_local_name(func, *id, Some(expr_id));
                if is_statement {
                    self.output.push_str(&format!("{} = ", name));
                    self.walk_inline_expression(func, *value);
                    self.output.push_str(";\n");
                } else {
                    self.output.push_str(&format!("({} = ", name));
                    self.walk_inline_expression(func, *value);
                    self.output.push_str(")");
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
                    self.walk_inline_expression(func, *if_true);
                    self.output.push_str(";\n");
                } else {
                    self.output.push_str("if ");
                    self.walk_inline_expression(func, *condition);
                    self.output.push_str(" { ");
                    self.walk_inline_expression(func, *if_true);
                    self.output.push_str(" } else { ");
                    self.walk_inline_expression(func, *if_false);
                    self.output.push_str(" }\n");
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
                self.output.push_str(";\n");
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
                self.walk_inline_expression(func, *target);
                self.output.push_str("(");
                for (i, op) in operands.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.walk_inline_expression(func, *op);
                }
                self.output.push_str(")\n");
            }
            ExpressionKind::Try {
                body, catch_bodies, ..
            } => {
                self.output.push_str("/* try */\n");
                self.walk_expression_ext(func, *body, false);
                for &cb in catch_bodies {
                    self.walk_expression_ext(func, cb, false);
                }
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
                    _ => "unary",
                };
                self.output.push_str(&format!("{}(", op_str));
                self.walk_inline_expression(func, *value);
                self.output.push_str(")");
            }
            ExpressionKind::LocalTee { id, value } => {
                let name = self.get_local_name(func, *id, Some(expr_id));
                self.output.push_str("(");
                self.output.push_str(&name);
                self.output.push_str(" = ");
                self.walk_inline_expression(func, *value);
                self.output.push_str(")");
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
            self.output.push_str(&format!("{} ", keyword));
            if let Some(n) = target_name {
                self.output.push_str(&format!("'{}: ", n));
            }
            self.walk_inline_expression(func, v);
            self.output.push_str(";\n");
        } else if let Some(n) = target_name {
            self.output.push_str(&format!("{} '{};\n", keyword, n));
        } else {
            self.output.push_str(&format!("{};\n", keyword));
        }
    }
}
