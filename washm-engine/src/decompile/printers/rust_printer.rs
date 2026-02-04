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

    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
    }

    fn walk_expression_ext(&mut self, func: &Function, expr_id: ExpressionId, is_return: bool) {
        if self.module.annotations.is_inlined(expr_id) {
            return;
        }

        self.write_indent();

        let expr = &self.module.expressions[expr_id];
        match &expr.kind {
            ExpressionKind::Block { name, list } => {
                let mut is_if = false;
                if let Some((condition, inverted)) = self.module.annotations.get_if_info(expr_id) {
                    is_if = true;
                    self.output.push_str("if ");
                    if inverted {
                        self.output.push_str("!");
                    }
                    self.walk_inline_expression(func, condition);
                    self.output.push_str(" ");
                } else if let Some(n) = name {
                    self.output.push_str(&format!("'{}: ", n));
                }

                self.output.push_str("{\n");
                self.indent += 1;

                let start_idx = if is_if { 1 } else { 0 };
                for (i, &child_id) in list.iter().enumerate().skip(start_idx) {
                    let child_is_last = i == list.len() - 1;
                    self.walk_expression_ext(func, child_id, is_return && child_is_last);
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
            // Add more cases as needed ...
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
                    crate::ir::ops::BinaryOp::EqInt32 | crate::ir::ops::BinaryOp::EqInt64 => "==",
                    crate::ir::ops::BinaryOp::NeInt32 | crate::ir::ops::BinaryOp::NeInt64 => "!=",
                    _ => "?",
                };
                self.output.push_str(&format!(" {} ", op_str));
                self.walk_inline_expression(func, *right);
                self.output.push_str(")");
            }
            _ => {
                self.output.push_str(&format!("/* {:?} */", expr.kind));
            }
        }
    }
}
