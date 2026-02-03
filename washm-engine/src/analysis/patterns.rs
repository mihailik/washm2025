use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::ops::{BinaryOp, UnaryOp};
use crate::ir::Literal;
use std::collections::HashMap;

/// Operations that can be matched in a Binary pattern
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternOp {
    Binary(BinaryOp),
    AnyOp,
}

impl From<BinaryOp> for PatternOp {
    fn from(op: BinaryOp) -> Self {
        PatternOp::Binary(op)
    }
}

/// Operations that can be matched in a Unary pattern
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternUnaryOp {
    Unary(UnaryOp),
    AnyOp,
}

impl From<UnaryOp> for PatternUnaryOp {
    fn from(op: UnaryOp) -> Self {
        PatternUnaryOp::Unary(op)
    }
}

/// A pattern to match against expressions
#[derive(Debug, Clone)]
pub enum Pattern {
    /// Match a constant
    Const(Literal),
    /// Match any constant
    AnyConst,
    /// Match a specific binary operator and its children
    Binary {
        op: PatternOp,
        left: Box<Pattern>,
        right: Box<Pattern>,
    },
    /// Match a specific unary operator and its child
    Unary {
        op: PatternUnaryOp,
        value: Box<Pattern>,
    },
    /// Match a select (ternary) operation
    Select {
        condition: Box<Pattern>,
        if_true: Box<Pattern>,
        if_false: Box<Pattern>,
    },
    /// Match any expression and capture it as a variable
    Var(&'static str),
    /// Match any expression but don't capture any variables
    Any,
}

/// Captures variables from a matched pattern
#[derive(Default)]
pub struct MatchEnv {
    vars: HashMap<&'static str, ExpressionId>,
    pub matched_op: Option<BinaryOp>,
    pub matched_unary_op: Option<UnaryOp>,
    pub left: Option<ExpressionId>,
    pub right: Option<ExpressionId>,
}

impl MatchEnv {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.vars.clear();
        self.matched_op = None;
        self.matched_unary_op = None;
        self.left = None;
        self.right = None;
    }

    pub fn get_op(&self) -> Option<BinaryOp> {
        self.matched_op
    }

    pub fn get_unary_op(&self) -> Option<UnaryOp> {
        self.matched_unary_op
    }

    pub fn insert(&mut self, name: &'static str, expr: ExpressionId) {
        self.vars.insert(name, expr);
    }

    pub fn get(&self, name: &'static str) -> Option<&ExpressionId> {
        self.vars.get(name)
    }

    pub fn get_const(
        &self,
        name: &'static str,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> Option<Literal> {
        self.get(name).and_then(|&id| {
            if let ExpressionKind::Const(lit) = &arena[id].kind {
                Some(*lit)
            } else {
                None
            }
        })
    }

    pub fn set_left(&mut self, expr: ExpressionId) {
        self.left = Some(expr);
    }

    pub fn set_right(&mut self, expr: ExpressionId) {
        self.right = Some(expr);
    }
}

impl Pattern {
    /// Helper to create a binary pattern
    pub fn binary(op: impl Into<PatternOp>, left: Pattern, right: Pattern) -> Self {
        Pattern::Binary {
            op: op.into(),
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    /// Helper to create a unary pattern
    pub fn unary(op: impl Into<PatternUnaryOp>, value: Pattern) -> Self {
        Pattern::Unary {
            op: op.into(),
            value: Box::new(value),
        }
    }

    /// Helper to create a select pattern
    pub fn select(condition: Pattern, if_true: Pattern, if_false: Pattern) -> Self {
        Pattern::Select {
            condition: Box::new(condition),
            if_true: Box::new(if_true),
            if_false: Box::new(if_false),
        }
    }

    /// Try to match expression against this pattern
    pub fn matches(
        &self,
        expr_id: ExpressionId,
        env: &mut MatchEnv,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> bool {
        let expr = &arena[expr_id];
        match (self, &expr.kind) {
            (Pattern::Any, _) => true,

            (Pattern::Const(target), ExpressionKind::Const(actual)) => target == actual,

            (Pattern::AnyConst, ExpressionKind::Const(_actual)) => {
                if env.left.is_none() {
                    env.set_left(expr_id);
                    env.insert("left", expr_id);
                } else if env.right.is_none() {
                    env.set_right(expr_id);
                    env.insert("right", expr_id);
                }
                true
            }

            (Pattern::Var(name), _) => {
                if let Some(existing) = env.get(name) {
                    if *existing == expr_id {
                        return true;
                    }
                    if let (ExpressionKind::Const(a), ExpressionKind::Const(b)) =
                        (&arena[*existing].kind, &expr.kind)
                    {
                        return a == b;
                    }
                    false
                } else {
                    env.insert(name, expr_id);
                    true
                }
            }

            (
                Pattern::Binary {
                    op,
                    left: p_left,
                    right: p_right,
                },
                ExpressionKind::Binary {
                    op: e_op,
                    left: e_left,
                    right: e_right,
                },
            ) => {
                let op_matches = match op {
                    PatternOp::Binary(target_op) => target_op == e_op,
                    PatternOp::AnyOp => {
                        env.matched_op = Some(*e_op);
                        true
                    }
                };

                if op_matches
                    && p_left.matches(*e_left, env, arena)
                    && p_right.matches(*e_right, env, arena)
                {
                    return true;
                }
                false
            }

            (
                Pattern::Unary { op, value: p_val },
                ExpressionKind::Unary {
                    op: e_op,
                    value: e_val,
                },
            ) => {
                let op_matches = match op {
                    PatternUnaryOp::Unary(target_op) => target_op == e_op,
                    PatternUnaryOp::AnyOp => {
                        env.matched_unary_op = Some(*e_op);
                        true
                    }
                };

                op_matches && p_val.matches(*e_val, env, arena)
            }

            (
                Pattern::Select {
                    condition: p_cond,
                    if_true: p_true,
                    if_false: p_false,
                },
                ExpressionKind::Select {
                    condition: e_cond,
                    if_true: e_true,
                    if_false: e_false,
                },
            ) => {
                p_cond.matches(*e_cond, env, arena)
                    && p_true.matches(*e_true, env, arena)
                    && p_false.matches(*e_false, env, arena)
            }

            _ => false,
        }
    }
}

pub struct Rule {
    pub pattern: Pattern,
    pub replacement: Box<
        dyn Fn(&MatchEnv, &mut IdArena<ExpressionMarker, Expression>) -> Option<ExpressionId>
            + Sync
            + Send,
    >,
}

pub struct PatternMatcher {
    pub rules: Vec<Rule>,
}

impl Default for PatternMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl PatternMatcher {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn add_rule<F>(&mut self, pattern: Pattern, replacement: F)
    where
        F: Fn(&MatchEnv, &mut IdArena<ExpressionMarker, Expression>) -> Option<ExpressionId>
            + 'static
            + Sync
            + Send,
    {
        self.rules.push(Rule {
            pattern,
            replacement: Box::new(replacement),
        });
    }

    pub fn simplify(
        &self,
        expr_id: ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) -> Option<ExpressionId> {
        let mut env = MatchEnv::new();

        for rule in &self.rules {
            env.clear();
            if rule.pattern.matches(expr_id, &mut env, arena) {
                if let Some(new_expr) = (rule.replacement)(&env, arena) {
                    return Some(new_expr);
                }
            }
        }

        None
    }
}
