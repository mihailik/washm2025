use crate::ir::expression::ExpressionId;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugLocation {
    pub file_index: u32,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Annotation {
    Loop(LoopType),
    Type(HighLevelType),
    Variable(VariableRole),
    LocalName(String),
    If {
        condition: ExpressionId,
        inverted: bool,
    },
    Inlined,                    // Tag for local.set that should be omitted
    InlinedValue(ExpressionId), // Tag for local.get that should be replaced by this expression
    Location(DebugLocation),
    ConstantGlobal(crate::ir::Literal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopType {
    For,
    While,
    DoWhile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighLevelType {
    Bool,
    Pointer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableRole {
    LoopIndex,
    BasePointer,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Annotations {
    pub loop_type: Option<LoopType>,
    pub high_level_type: Option<HighLevelType>,
    pub role: Option<VariableRole>,
    pub local_name: Option<String>,
    pub if_info: Option<(ExpressionId, bool)>,
    pub inlined: bool,
    pub inlined_value: Option<ExpressionId>,
    pub location: Option<DebugLocation>,
    pub constant_global: Option<crate::ir::Literal>,
}

#[derive(Debug, Default, Clone)]
pub struct AnnotationStore {
    inner: HashMap<ExpressionId, Annotations>,
}

impl IntoIterator for AnnotationStore {
    type Item = (ExpressionId, Annotations);
    type IntoIter = std::collections::hash_map::IntoIter<ExpressionId, Annotations>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl AnnotationStore {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    pub fn insert(&mut self, expr: ExpressionId, annotation: Annotation) {
        let entry = self.inner.entry(expr).or_default();
        match annotation {
            Annotation::Loop(lt) => entry.loop_type = Some(lt),
            Annotation::Type(ht) => entry.high_level_type = Some(ht),
            Annotation::Variable(r) => entry.role = Some(r),
            Annotation::LocalName(n) => entry.local_name = Some(n),
            Annotation::If {
                condition,
                inverted,
            } => entry.if_info = Some((condition, inverted)),
            Annotation::Inlined => entry.inlined = true,
            Annotation::InlinedValue(val) => entry.inlined_value = Some(val),
            Annotation::Location(loc) => entry.location = Some(loc),
            Annotation::ConstantGlobal(lit) => entry.constant_global = Some(lit),
        }
    }

    pub fn set_location(&mut self, expr: ExpressionId, location: DebugLocation) {
        self.inner.entry(expr).or_default().location = Some(location);
    }

    pub fn get_location(&self, expr: ExpressionId) -> Option<DebugLocation> {
        self.inner.get(&expr).and_then(|a| a.location)
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }

    pub fn clear_locations(&mut self) {
        for info in self.inner.values_mut() {
            info.location = None;
        }
    }

    pub fn get(&self, expr: ExpressionId) -> Option<Annotations> {
        self.inner.get(&expr).cloned()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// If this is an 'If' block, returns information about the condition.
    pub fn get_if_info(&self, expr: ExpressionId) -> Option<(ExpressionId, bool)> {
        self.inner.get(&expr).and_then(|a| a.if_info)
    }

    /// Returns the high-level loop type if this is a loop.
    pub fn get_loop_type(&self, expr: ExpressionId) -> Option<LoopType> {
        self.inner.get(&expr).and_then(|a| a.loop_type)
    }

    /// Returns the high-level type if this expression has one.
    pub fn get_high_level_type(&self, expr: ExpressionId) -> Option<HighLevelType> {
        self.inner.get(&expr).and_then(|a| a.high_level_type)
    }

    /// Returns true if this expression should be treated as 'inlined' (omitted as a statement).
    pub fn is_inlined(&self, expr: ExpressionId) -> bool {
        self.inner.get(&expr).is_some_and(|a| a.inlined)
    }

    /// Returns the source expression if this is an inlined value.
    pub fn get_inlined_value(&self, expr: ExpressionId) -> Option<ExpressionId> {
        self.inner.get(&expr).and_then(|a| a.inlined_value)
    }

    pub fn get_constant_global(&self, expr: ExpressionId) -> Option<crate::ir::Literal> {
        self.inner.get(&expr).and_then(|a| a.constant_global)
    }

    pub fn get_local_name(&self, expr: ExpressionId) -> Option<&str> {
        self.inner.get(&expr).and_then(|a| a.local_name.as_deref())
    }
}
