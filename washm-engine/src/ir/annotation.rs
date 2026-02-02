use crate::ir::expression::ExprRef;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugLocation {
    pub file_index: u32,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Annotation<'a> {
    Loop(LoopType),
    Type(HighLevelType),
    Variable(VariableRole),
    LocalName(&'a str),
    If {
        condition: ExprRef<'a>,
        inverted: bool,
    },
    Inlined,                   // Tag for local.set that should be omitted
    InlinedValue(ExprRef<'a>), // Tag for local.get that should be replaced by this expression
    Location(DebugLocation),
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Annotations<'a> {
    pub loop_type: Option<LoopType>,
    pub high_level_type: Option<HighLevelType>,
    pub role: Option<VariableRole>,
    pub local_name: Option<&'a str>,
    pub if_info: Option<(ExprRef<'a>, bool)>,
    pub inlined: bool,
    pub inlined_value: Option<ExprRef<'a>>,
    pub location: Option<DebugLocation>,
}

#[derive(Debug, Default)]
pub struct AnnotationStore<'a> {
    inner: HashMap<ExprRef<'a>, Annotations<'a>>,
}

impl<'a> IntoIterator for AnnotationStore<'a> {
    type Item = (ExprRef<'a>, Annotations<'a>);
    type IntoIter = std::collections::hash_map::IntoIter<ExprRef<'a>, Annotations<'a>>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<'a> AnnotationStore<'a> {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    pub fn insert(&mut self, expr: ExprRef<'a>, annotation: Annotation<'a>) {
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
        }
    }

    pub fn set_location(&mut self, expr: ExprRef<'a>, location: DebugLocation) {
        self.inner.entry(expr).or_default().location = Some(location);
    }

    pub fn get_location(&self, expr: ExprRef<'a>) -> Option<DebugLocation> {
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

    pub fn get(&self, expr: ExprRef<'a>) -> Option<Annotations<'a>> {
        self.inner.get(&expr).copied()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// If this is an 'If' block, returns information about the condition.
    pub fn get_if_info(&self, expr: ExprRef<'a>) -> Option<(ExprRef<'a>, bool)> {
        self.inner.get(&expr).and_then(|a| a.if_info)
    }

    /// Returns the high-level loop type if this is a loop.
    pub fn get_loop_type(&self, expr: ExprRef<'a>) -> Option<LoopType> {
        self.inner.get(&expr).and_then(|a| a.loop_type)
    }

    /// Returns the high-level type if this expression has one.
    pub fn get_high_level_type(&self, expr: ExprRef<'a>) -> Option<HighLevelType> {
        self.inner.get(&expr).and_then(|a| a.high_level_type)
    }

    /// Returns true if this expression should be treated as 'inlined' (omitted as a statement).
    pub fn is_inlined(&self, expr: ExprRef<'a>) -> bool {
        self.inner.get(&expr).map_or(false, |a| a.inlined)
    }

    /// Returns the source expression if this is an inlined value.
    pub fn get_inlined_value(&self, expr: ExprRef<'a>) -> Option<ExprRef<'a>> {
        self.inner.get(&expr).and_then(|a| a.inlined_value)
    }

    pub fn get_local_name(&self, expr: ExprRef<'a>) -> Option<&'a str> {
        self.inner.get(&expr).and_then(|a| a.local_name)
    }
}
