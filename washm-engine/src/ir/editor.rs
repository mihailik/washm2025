use crate::collections::id_arena::IdArena;
use crate::ir::annotation::AnnotationStore;
use crate::ir::expression::{
    Expression, ExpressionId, ExpressionKind, ExpressionMarker, IrBuilder,
};
use crate::ir::module::{Function, LocalId, Module};
use crate::ir::visitor::Visitor;
use crate::ir::Type;

pub struct ModuleEditor<'a> {
    pub module: &'a mut Module,
}

impl<'a> ModuleEditor<'a> {
    pub fn new(module: &'a mut Module) -> Self {
        Self { module }
    }

    pub fn edit_function(&mut self, id: crate::ir::module::FuncId) -> FunctionEditor<'_> {
        let (
            expressions,
            _types,
            functions,
            _globals,
            _exports,
            _imports,
            _tables,
            _memories,
            _elements,
            _data,
            _tags,
            _strings,
            annotations,
        ) = self.module.parts_mut();
        let func = functions.get_mut(id.cast()).expect("Function not found");
        FunctionEditor {
            func,
            expressions,
            annotations,
        }
    }
}

pub struct FunctionEditor<'a> {
    pub func: &'a mut Function,
    pub expressions: &'a mut IdArena<ExpressionMarker, Expression>,
    pub annotations: &'a mut AnnotationStore,
}

impl<'a> FunctionEditor<'a> {
    pub fn builder(&mut self) -> IrBuilder<'_> {
        IrBuilder::with_annotations(self.expressions, self.annotations)
    }

    /// Wraps the function body in a block and allows adding instructions before/after.
    pub fn wrap_body<F>(&mut self, wrapper: F)
    where
        F: FnOnce(&mut FunctionEditor, ExpressionId) -> Vec<ExpressionId>,
    {
        let results = self.func.results;
        if let Some(&mut body) = self.func.body_mut() {
            let mut editor = FunctionEditor {
                func: self.func,
                expressions: self.expressions,
                annotations: self.annotations,
            };
            let new_list = wrapper(&mut editor, body);

            let mut builder = editor.builder();
            builder.copy_location(body);
            let new_body = builder.block("", new_list, results);
            self.func.set_body(new_body);
        }
    }

    /// Adds a new local variable to the function and returns its ID.
    pub fn add_local(&mut self, type_: Type) -> LocalId {
        self.func.add_var(type_)
    }

    /// Intercepts the function entry by prepending a call to another function.
    pub fn intercept_call(&mut self, logger_id: crate::ir::module::FuncId) -> &mut Self {
        let results = self.func.results;
        if let Some(body) = self.func.body() {
            let call = {
                let mut builder = self.builder();
                builder.copy_location(body);
                builder.call(logger_id, vec![], Type::NONE, false)
            };

            // If body is a block, just prepend.
            let expr = &mut self.expressions[body];
            if let ExpressionKind::Block { list, .. } = &mut expr.kind {
                list.insert(0, call);
            } else {
                // Otherwise wrap in a new block.
                let mut builder = self.builder();
                builder.copy_location(body);
                let new_body = builder.block("", vec![call, body], results);
                self.func.set_body(new_body);
            }
        }
        self
    }

    /// Redirects calls from one function to another.
    pub fn redirect_calls(
        &mut self,
        old_target: crate::ir::module::FuncId,
        new_target: crate::ir::module::FuncId,
    ) -> &mut Self {
        if let Some(mut body) = self.func.body() {
            let mut visitor = CallRedirector {
                old_target,
                new_target,
            };
            visitor.visit_expression(&mut body, self.expressions);
            if let Some(body_ref) = self.func.body_mut() {
                visitor.visit_expression(body_ref, self.expressions);
            }
        }
        self
    }

    /// Visits all expressions in the function body and allows surgical edits.
    pub fn edit_each<F>(&mut self, mut f: F) -> &mut Self
    where
        F: FnMut(&mut SurgicalHandle),
    {
        if let Some(mut body) = self.func.body() {
            surgical_visit(&mut body, self.expressions, self.annotations, &mut f);
            if let Some(body_ref) = self.func.body_mut() {
                surgical_visit(body_ref, self.expressions, self.annotations, &mut f);
            }
        }
        self
    }
}

fn surgical_visit<F>(
    expr_id: &mut ExpressionId,
    arena: &mut IdArena<ExpressionMarker, Expression>,
    annotations: &mut AnnotationStore,
    f: &mut F,
) where
    F: FnMut(&mut SurgicalHandle),
{
    let mut handle = SurgicalHandle {
        current_id: expr_id,
        arena,
        annotations,
    };
    f(&mut handle);

    let mut kind = std::mem::replace(
        &mut handle.arena[*handle.current_id].kind,
        ExpressionKind::Nop,
    );
    kind.for_each_child_mut(|child_id| {
        surgical_visit(child_id, handle.arena, handle.annotations, f);
    });
    handle.arena[*handle.current_id].kind = kind;
}

pub struct SurgicalHandle<'a> {
    pub current_id: &'a mut ExpressionId,
    pub arena: &'a mut IdArena<ExpressionMarker, Expression>,
    pub annotations: &'a mut AnnotationStore,
}

impl<'a> SurgicalHandle<'a> {
    pub fn replace_with(&mut self, new_id: ExpressionId) {
        *self.current_id = new_id;
    }

    pub fn builder(&mut self) -> IrBuilder<'_> {
        IrBuilder::with_annotations(self.arena, self.annotations)
    }
}

struct CallRedirector {
    old_target: crate::ir::module::FuncId,
    new_target: crate::ir::module::FuncId,
}

impl Visitor for CallRedirector {
    fn visit_expression(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let expr = &mut arena[*expr_id];
        if let ExpressionKind::Call { target, .. } = &mut expr.kind {
            if *target == self.old_target {
                *target = self.new_target;
            }
        }
        self.visit_children(expr_id, arena);
    }
}
