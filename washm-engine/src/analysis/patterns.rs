#![allow(clippy::type_complexity)]
use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::ops::{BinaryOp, UnaryOp};
use crate::ir::Literal;
use std::collections::HashMap;

/// Captured environment during pattern matching
/// Maps variable names (from Pattern::Var) to the matched sub-expressions
pub struct MatchEnv<'a> {
    vars: HashMap<&'static str, ExprRef<'a>>,
    op: Option<BinaryOp>,
    unary_op: Option<UnaryOp>,
    left: Option<ExprRef<'a>>,
    right: Option<ExprRef<'a>>,
}

impl<'a> MatchEnv<'a> {
    pub fn new() -> Self {
        Self {
            vars: HashMap::new(),
            op: None,
            unary_op: None,
            left: None,
            right: None,
        }
    }

    pub fn get(&self, name: &str) -> Option<&ExprRef<'a>> {
        self.vars.get(name)
    }

    pub fn get_const(&self, name: &str) -> Option<Literal> {
        if let Some(expr) = self.vars.get(name) {
            if let ExpressionKind::Const(lit) = expr.kind {
                return Some(lit);
            }
        }
        if name == "left" {
            if let Some(expr) = self.left {
                if let ExpressionKind::Const(lit) = expr.kind {
                    return Some(lit);
                }
            }
        }
        if name == "right" {
            if let Some(expr) = self.right {
                if let ExpressionKind::Const(lit) = expr.kind {
                    return Some(lit);
                }
            }
        }
        None
    }

    pub fn get_op(&self) -> Option<BinaryOp> {
        self.op
    }

    pub fn get_unary_op(&self) -> Option<UnaryOp> {
        self.unary_op
    }

    pub fn insert(&mut self, name: &'static str, expr: ExprRef<'a>) {
        self.vars.insert(name, expr);
    }

    pub fn set_op(&mut self, op: BinaryOp) {
        self.op = Some(op);
    }

    pub fn set_unary_op(&mut self, op: UnaryOp) {
        self.unary_op = Some(op);
    }

    pub fn set_left(&mut self, expr: ExprRef<'a>) {
        self.left = Some(expr);
    }

    pub fn set_right(&mut self, expr: ExprRef<'a>) {
        self.right = Some(expr);
    }

    pub fn clear(&mut self) {
        self.vars.clear();
        self.op = None;
        self.unary_op = None;
        self.left = None;
        self.right = None;
    }
}

/// Pattern matching DSL for simplification rules
#[derive(Debug, Clone)]
pub enum Pattern {
    /// Match any expression (wildcard)
    Any,
    /// Match specific constant value
    Const(Literal),
    /// Match any constant
    AnyConst,
    /// Match and capture expression into a variable
    Var(&'static str),
    /// Match binary operation
    Binary {
        op: PatternOp,
        left: Box<Pattern>,
        right: Box<Pattern>,
    },
    /// Match unary operation
    Unary {
        op: PatternUnaryOp,
        value: Box<Pattern>,
    },
    /// Match select expression
    Select {
        condition: Box<Pattern>,
        if_true: Box<Pattern>,
        if_false: Box<Pattern>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternOp {
    Op(BinaryOp),
    AnyOp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternUnaryOp {
    Op(UnaryOp),
    AnyOp,
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
    pub fn matches<'a>(&self, expr: ExprRef<'a>, env: &mut MatchEnv<'a>) -> bool {
        match (self, &expr.kind) {
            (Pattern::Any, _) => true,

            (Pattern::Const(target), ExpressionKind::Const(actual)) => target == actual,

            (Pattern::AnyConst, ExpressionKind::Const(_actual)) => {
                // To make the constant available in the replacement function, we can
                // store it in the env with a conventional name. This is a bit of a
                // hack, but it works.
                if env.left.is_none() {
                    env.set_left(expr);
                    env.insert("left", expr);
                } else if env.right.is_none() {
                    env.set_right(expr);
                    env.insert("right", expr);
                }
                true
            }

            (Pattern::Var(name), _) => {
                if let Some(existing) = env.get(name) {
                    // Match if pointers are the same OR if they represent the same value (for literals/simple kinds)
                    if existing.as_ptr() == expr.as_ptr() {
                        return true;
                    }
                    // For now, let's keep it simple: pointer equality or value equality for constants.
                    // This prevents capturing (local.get 0) and (local.get 0) as different if they are different pointers.
                    match (&existing.kind, &expr.kind) {
                        (ExpressionKind::Const(a), ExpressionKind::Const(b)) => a == b,
                        (
                            ExpressionKind::LocalGet { index: a },
                            ExpressionKind::LocalGet { index: b },
                        ) => a == b,
                        _ => false,
                    }
                } else {
                    env.insert(name, expr);
                    true
                }
            }

            (
                Pattern::Binary {
                    op: p_op,
                    left: p_left,
                    right: p_right,
                },
                ExpressionKind::Binary {
                    op: e_op,
                    left: e_left,
                    right: e_right,
                },
            ) => {
                let op_matches = match p_op {
                    PatternOp::Op(op) => op == e_op,
                    PatternOp::AnyOp => {
                        env.set_op(*e_op);
                        true
                    }
                };
                op_matches && p_left.matches(*e_left, env) && p_right.matches(*e_right, env)
            }

            (
                Pattern::Unary {
                    op: p_op,
                    value: p_value,
                },
                ExpressionKind::Unary {
                    op: e_op,
                    value: e_value,
                },
            ) => {
                let op_matches = match p_op {
                    PatternUnaryOp::Op(op) => op == e_op,
                    PatternUnaryOp::AnyOp => {
                        env.set_unary_op(*e_op);
                        true
                    }
                };
                op_matches && p_value.matches(*e_value, env)
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
                p_cond.matches(*e_cond, env)
                    && p_true.matches(*e_true, env)
                    && p_false.matches(*e_false, env)
            }

            _ => false,
        }
    }
}

impl From<BinaryOp> for PatternOp {
    fn from(op: BinaryOp) -> Self {
        PatternOp::Op(op)
    }
}

impl From<UnaryOp> for PatternUnaryOp {
    fn from(op: UnaryOp) -> Self {
        PatternUnaryOp::Op(op)
    }
}

use bumpalo::Bump;

/// A simplification rule
pub struct Rule {
    pub pattern: Pattern,
    // The replacement function takes an Env and an Arena, and returns a new expression
    pub replacement:
        Box<dyn for<'a> Fn(&MatchEnv<'a>, &'a Bump) -> Option<ExprRef<'a>> + Sync + Send>,
}

/// Engine to apply simplification rules
pub struct PatternMatcher {
    rules: Vec<Rule>,
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
        F: for<'a> Fn(&MatchEnv<'a>, &'a Bump) -> Option<ExprRef<'a>> + 'static + Sync + Send,
    {
        self.rules.push(Rule {
            pattern,
            replacement: Box::new(replacement),
        });
    }

    /// Try to simplify an expression using registered rules.
    pub fn simplify<'a>(&self, expr: ExprRef<'a>, arena: &'a Bump) -> Option<ExprRef<'a>> {
        let mut env = MatchEnv::new();

        for rule in &self.rules {
            env.clear();
            if rule.pattern.matches(expr, &mut env) {
                if let Some(new_expr) = (rule.replacement)(&env, arena) {
                    return Some(new_expr);
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::Type;
    use bumpalo::Bump;

    #[test]
    fn test_match_const() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let c42 = builder.const_(Literal::I32(42));
        let c100 = builder.const_(Literal::I32(100));

        let p42 = Pattern::Const(Literal::I32(42));
        let p_any_const = Pattern::AnyConst;

        let mut env = MatchEnv::new();

        assert!(p42.matches(c42, &mut env));
        env.clear();
        assert!(!p42.matches(c100, &mut env));

        env.clear();
        assert!(p_any_const.matches(c42, &mut env));
        env.clear();
        assert!(p_any_const.matches(c100, &mut env));
    }

    #[test]
    fn test_match_var() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let c42 = builder.const_(Literal::I32(42));
        let p_x = Pattern::Var("x");

        let mut env = MatchEnv::new();

        assert!(p_x.matches(c42, &mut env));
        assert_eq!(env.get("x").unwrap().as_ptr(), c42.as_ptr());
    }

    #[test]
    fn test_match_binary_structure() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // (42 + 100)
        let left = builder.const_(Literal::I32(42));
        let right = builder.const_(Literal::I32(100));
        let add = builder.binary(BinaryOp::AddInt32, left, right, Type::I32);

        // Pattern: (AnyConst + AnyConst)
        let p = Pattern::binary(BinaryOp::AddInt32, Pattern::AnyConst, Pattern::AnyConst);

        let mut env = MatchEnv::new();
        assert!(p.matches(add, &mut env));

        // Pattern: (x + y)
        let p_vars = Pattern::binary(BinaryOp::AddInt32, Pattern::Var("x"), Pattern::Var("y"));

        env.clear();
        assert!(p_vars.matches(add, &mut env));
        assert_eq!(env.get("x").unwrap().as_ptr(), left.as_ptr());
        assert_eq!(env.get("y").unwrap().as_ptr(), right.as_ptr());
    }

    #[test]
    fn test_match_repeated_var() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let c42 = builder.const_(Literal::I32(42));
        let c100 = builder.const_(Literal::I32(100));

        // (42 + 42)
        let add_same = builder.binary(BinaryOp::AddInt32, c42, c42, Type::I32);
        // (42 + 100)
        let add_diff = builder.binary(BinaryOp::AddInt32, c42, c100, Type::I32);

        // Pattern: x + x
        let p = Pattern::binary(BinaryOp::AddInt32, Pattern::Var("x"), Pattern::Var("x"));

        let mut env = MatchEnv::new();
        assert!(p.matches(add_same, &mut env));

        env.clear();
        assert!(!p.matches(add_diff, &mut env));
    }

    #[test]
    fn test_simple_replacement() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // x + 0 -> x
        let x = builder.const_(Literal::I32(42));
        let zero = builder.const_(Literal::I32(0));
        let add = builder.binary(BinaryOp::AddInt32, x, zero, Type::I32);

        let mut matcher = PatternMatcher::new();
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AddInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(0)),
            ),
            |env, _| env.get("x").copied(),
        );

        let simplified = matcher.simplify(add, &bump);
        assert!(simplified.is_some());
        assert_eq!(simplified.unwrap().as_ptr(), x.as_ptr());
    }
}
