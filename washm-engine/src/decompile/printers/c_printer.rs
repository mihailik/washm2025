use crate::ir::expression::{ExpressionId, ExpressionKind};
use crate::ir::module::{Function, FunctionKind, Module};
use crate::ir::Type;

/// The CPrinter converts the annotated IR into human-readable C code.
pub struct CPrinter<'m> {
    pub module: &'m Module,
    output: String,
    indent: usize,
}

impl<'m> CPrinter<'m> {
    pub fn new(module: &'m Module) -> Self {
        Self {
            module,
            output: String::new(),
            indent: 0,
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
            .push_str("// Decompiled to C from WebAssembly\n\n");
        self.output
            .push_str("#include <stdint.h>\n#include <stdbool.h>\n\n");

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
            return "void".to_string();
        }
        elements
            .iter()
            .map(|&t| self.type_name(t))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn type_name(&self, ty: Type) -> String {
        match ty {
            Type::NONE => "void".to_string(),
            Type::I32 => "int32_t".to_string(),
            Type::I64 => "int64_t".to_string(),
            Type::F32 => "float".to_string(),
            Type::F64 => "double".to_string(),
            _ => format!("{:?}", ty).to_lowercase(),
        }
    }

    fn print_function(&mut self, func: &Function) {
        if func.results == Type::NONE {
            self.output.push_str("void ");
        } else {
            self.output
                .push_str(&format!("{} ", self.type_name(func.results)));
        }

        self.output.push_str(&format!("{}(", func.name));

        let param_types = func.params.tuple_elements();
        if let FunctionKind::Local { param_ids, .. } = &func.kind {
            if param_types.is_empty() {
                self.output.push_str("void");
            } else {
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
                        .push_str(&format!("{} {}", self.type_name(ty), name));
                }
            }
        } else {
            if param_types.is_empty() {
                self.output.push_str("void");
            } else {
                for (i, &ty) in param_types.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.output
                        .push_str(&format!("{} p{}", self.type_name(ty), i));
                }
            }
        }

        self.output.push_str(")");

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
                        self.output
                            .push_str(&format!("{} {} = 0;\n", self.type_name(ty), name));
                    }
                }

                self.walk_expression_ext(func, *body, true);

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

    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
    }

    fn walk_expression_ext(&mut self, func: &Function, expr_id: ExpressionId, is_statement: bool) {
        if self.module.annotations.is_inlined(expr_id) {
            return;
        }

        let expr = &self.module.expressions[expr_id];
        let is_return =
            is_statement && self.is_last_in_func(func, expr_id) && func.results != Type::NONE;

        if is_statement {
            self.write_indent();
        }

        // If this expression is in a return position and is printable as an inline
        // expression, print it as `return <expr>;` and avoid the generic fallback.
        if is_return {
            match &expr.kind {
                ExpressionKind::Const(_)
                | ExpressionKind::LocalGet { .. }
                | ExpressionKind::GlobalGet { .. }
                | ExpressionKind::LocalTee { .. }
                | ExpressionKind::Binary { .. }
                | ExpressionKind::Unary { .. }
                | ExpressionKind::Load { .. }
                | ExpressionKind::Call { .. }
                | ExpressionKind::Select { .. } => {
                    self.output.push_str("return ");
                    self.walk_inline_expression(func, expr_id);
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
                    self.output.push_str("if (");
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
                    self.output.push_str(") ");
                } else if let Some(n) = name {
                    self.output.push_str(&format!("/* block: {} */ ", n));
                }

                self.output.push_str("{\n");
                self.indent += 1;

                let start_idx = if is_if { 1 } else { 0 };
                for &child_id in list.iter().skip(start_idx) {
                    self.walk_expression_ext(func, child_id, true);
                }

                self.indent -= 1;
                self.write_indent();
                self.output.push_str("}\n");

                if let Some(n) = name {
                    // Only emit an end label if some Break targets this block name
                    let mut found = false;
                    for (_id, expr) in self.module.expressions.iter() {
                        if let ExpressionKind::Break { name: bn, .. } = &expr.kind {
                            if bn == n {
                                found = true;
                                break;
                            }
                        }
                    }
                    if found {
                        let safe = Self::sanitize_label(n);
                        self.output
                            .push_str(&format!("{}: ;\n", format!("{}_end", safe)));
                    }
                }
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
                    if is_statement {
                        self.output.push_str(";\n");
                    }
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
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
                ..
            } => {
                self.output.push_str("if (");
                self.walk_inline_expression(func, *condition);
                self.output.push_str(") ");
                self.walk_expression_ext(func, *if_true, true);
                if let Some(f) = if_false {
                    // Omit printing a pointless `else` when the branch is an explicit NOP
                    match &self.module.expressions[*f].kind {
                        ExpressionKind::Nop => {}
                        _ => {
                            self.write_indent();
                            self.output.push_str("else ");
                            self.walk_expression_ext(func, *f, true);
                        }
                    }
                }
            }
            ExpressionKind::Loop { name, body } => {
                if let Some(n) = name {
                    self.output.push_str(&format!("/* loop: {} */ ", n));
                }
                self.output.push_str("while (1) {\n");
                self.indent += 1;
                self.walk_expression_ext(func, *body, true);
                self.indent -= 1;
                self.write_indent();
                self.output.push_str("}\n");
            }
            ExpressionKind::Break {
                name,
                condition,
                value,
            } => {
                // Lower breaks to goto labels for C output. Use sanitized <name>_end labels
                if name.is_empty() {
                    // Unnamed break; fall back to comment
                    if let Some(v) = value {
                        self.output.push_str("/* break with value: ");
                        self.walk_inline_expression(func, *v);
                        self.output.push_str(" */;\n");
                    } else {
                        self.output.push_str("/* break */;\n");
                    }
                } else {
                    let safe = Self::sanitize_label(name);
                    let label = format!("{}_end", safe);
                    if let Some(cond) = condition {
                        self.output.push_str("if (");
                        self.walk_inline_expression(func, *cond);
                        self.output.push_str(") ");
                        if value.is_some() {
                            self.output.push_str("{ /* break with value */ goto ");
                            self.output.push_str(&label);
                            self.output.push_str("; }\n");
                        } else {
                            self.output.push_str("{ goto ");
                            self.output.push_str(&label);
                            self.output.push_str("; }\n");
                        }
                    } else {
                        if value.is_some() {
                            self.output.push_str("/* break with value */ goto ");
                            self.output.push_str(&label);
                            self.output.push_str(";\n");
                        } else {
                            self.output.push_str("goto ");
                            self.output.push_str(&label);
                            self.output.push_str(";\n");
                        }
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
                    self.output.push_str("(");
                    self.output.push_str(&format!("{} = ", name));
                    self.walk_inline_expression(func, *value);
                    self.output.push_str(")");
                }
            }
            ExpressionKind::Nop => {
                if is_statement {
                    self.output.push_str("/* nop */;\n");
                }
            }
            ExpressionKind::Unreachable => {
                self.output.push_str("/* unreachable */;\n");
            }
            ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            } => {
                if is_statement {
                    self.walk_inline_expression(func, *if_true);
                    self.output.push_str(" = ");
                    self.walk_inline_expression(func, *if_false);
                    self.output.push_str("; /* select */\n");
                } else {
                    self.output.push_str("(");
                    self.walk_inline_expression(func, *condition);
                    self.output.push_str(" ? ");
                    self.walk_inline_expression(func, *if_true);
                    self.output.push_str(" : ");
                    self.walk_inline_expression(func, *if_false);
                    self.output.push_str(")");
                }
            }
            ExpressionKind::Store {
                ptr,
                value,
                bytes: _,
                offset: _,
                align: _,
                memory: _,
            } => {
                self.walk_inline_expression(func, *ptr);
                self.output.push_str(" = ");
                self.walk_inline_expression(func, *value);
                self.output.push_str(";\n");
            }
            ExpressionKind::Load {
                ptr,
                bytes: _,
                signed: _,
                offset: _,
                align: _,
                memory: _,
            } => {
                if is_statement {
                    self.output.push_str("/* load */;\n");
                } else {
                    self.output.push_str("(");
                    self.walk_inline_expression(func, *ptr);
                    self.output.push_str(")");
                }
            }
            ExpressionKind::GlobalGet { id } => {
                if let Some(g) = self.module.globals.get(id.cast()) {
                    self.output.push_str(&g.name);
                } else {
                    self.output.push_str(&format!("global_{}", id.index()));
                }
                if is_statement {
                    self.output.push_str(";\n");
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
            ExpressionKind::Switch {
                names,
                default,
                condition,
                value: _,
            } => {
                self.output.push_str("switch (");
                self.walk_inline_expression(func, *condition);
                self.output.push_str(") { /* targets: ");
                self.output.push_str(&names.join(", "));
                self.output.push_str(" default: ");
                self.output.push_str(default);
                self.output.push_str(" */ }\n");
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
                if is_statement {
                    self.output.push_str(";\n");
                }
            }
            ExpressionKind::MemoryGrow { memory: _, delta } => {
                self.output.push_str("/* memory.grow */ ");
                self.walk_inline_expression(func, *delta);
                if is_statement {
                    self.output.push_str(";\n");
                }
            }
            ExpressionKind::MemorySize { memory: _ } => {
                self.output.push_str("/* memory.size */");
                if is_statement {
                    self.output.push_str(";\n");
                }
            }
            ExpressionKind::Drop { value } => {
                self.walk_inline_expression(func, *value);
                if is_statement {
                    self.output.push_str(";\n");
                }
            }
            ExpressionKind::Try {
                name: _,
                body,
                catch_tags: _,
                catch_bodies,
                delegate: _,
            } => {
                self.output.push_str("/* try */\n");
                self.walk_expression_ext(func, *body, true);
                for cb in catch_bodies {
                    self.walk_expression_ext(func, *cb, true);
                }
            }
            ExpressionKind::Throw { tag, operands: _ } => {
                self.output.push_str(&format!("/* throw {} */", tag));
                if is_statement {
                    self.output.push_str(";\n");
                }
            }
            _ => {
                self.output
                    .push_str(&format!("/* unsupported expr: {:?} */", expr.kind));
                if is_statement {
                    self.output.push_str(";\n");
                }
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
                    crate::ir::ops::BinaryOp::EqInt32 | crate::ir::ops::BinaryOp::EqInt64 => "==",
                    crate::ir::ops::BinaryOp::NeInt32 | crate::ir::ops::BinaryOp::NeInt64 => "!=",
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
                self.output.push_str(&format!("{} = ", name));
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
                self.output.push_str("(");
                self.walk_inline_expression(func, *condition);
                self.output.push_str(" ? ");
                self.walk_inline_expression(func, *if_true);
                self.output.push_str(" : ");
                self.walk_inline_expression(func, *if_false);
                self.output.push_str(")");
            }
            ExpressionKind::Load { ptr, .. } => {
                self.output.push_str("(");
                self.walk_inline_expression(func, *ptr);
                self.output.push_str(") /* load */");
            }
            ExpressionKind::Store { ptr, value, .. } => {
                self.walk_inline_expression(func, *ptr);
                self.output.push_str(" = ");
                self.walk_inline_expression(func, *value);
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
            ExpressionKind::TableGet { table, index } => {
                self.output.push_str(&format!("table_{}[", table.index()));
                self.walk_inline_expression(func, *index);
                self.output.push_str("]");
            }
            ExpressionKind::TupleExtract { tuple, index } => {
                self.output.push_str("/* tuple.extract */ ");
                self.walk_inline_expression(func, *tuple);
                self.output.push_str(&format!(".{}", index));
            }
            ExpressionKind::RefNull { type_: _ } => {
                self.output.push_str("NULL /* ref.null */");
            }
            ExpressionKind::RefIsNull { value } => {
                self.output.push_str("(/* ref.is_null */ ");
                self.walk_inline_expression(func, *value);
                self.output.push_str(")");
            }
            ExpressionKind::RefEq { left, right } => {
                self.walk_inline_expression(func, *left);
                self.output.push_str(" == ");
                self.walk_inline_expression(func, *right);
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
}
