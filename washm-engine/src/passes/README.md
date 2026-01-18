# Writing Optimization Passes

This guide explains how to write optimization passes for the Binaryen Rust IR.

## Overview

Passes transform WebAssembly IR to optimize code. The infrastructure provides:
- Arena-based memory allocation (Bump allocator)
- Expression creation helpers
- Visitor pattern for tree traversal
- Safe lifetime management

## Basic Pass Structure

```rust
use crate::expression::{ExprRef, Expression, ExpressionKind};
use crate::module::Module;
use crate::pass::Pass;
use crate::visitor::Visitor;

pub struct MyPass;

impl Pass for MyPass {
    fn name(&self) -> &str {
        "my-pass"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        // Access allocator
        let allocator = module.allocator();
        
        // Transform each function
        for func in &mut module.functions {
            if let Some(body) = &mut func.body {
                let mut transformer = MyTransformer { allocator };
                transformer.visit(body);
            }
        }
    }
}
```

## Accessing the Allocator

Every `Module` provides access to its bump allocator:

```rust
let allocator = module.allocator();
```

This allocator has the same lifetime as the module and can be used to create new expressions.

## Creating New Expressions

Use the static helper methods on `Expression`:

### Basic Operations

```rust
// Create a nop
let nop = Expression::nop(allocator);

// Create a constant
let const_val = Expression::const_expr(allocator, Literal::I32(42), Type::I32);

// Create a block
let mut list = BumpVec::new_in(allocator);
list.push(expr1);
list.push(expr2);
let block = Expression::block(allocator, None, list, Type::I32);
```

### Local Operations

```rust
// local.get
let get = Expression::local_get(allocator, 0, Type::I32);

// local.set
let set = Expression::local_set(allocator, 0, value);

// local.tee
let tee = Expression::local_tee(allocator, 0, value, Type::I32);
```

## Pattern: Transforming Expressions

The Visitor pattern is used for tree traversal. Here's how to replace an expression:

```rust
struct MyTransformer<'a> {
    allocator: &'a bumpalo::Bump,
}

impl<'a> Visitor<'a> for MyTransformer<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        // Check if this is the pattern we want to transform
        if let ExpressionKind::SomePattern { field1, field2 } = &expr.kind {
            // Extract data we need
            let data1 = *field1;
            let data2 = *field2;
            let expr_type = expr.type_;
            
            // Create new expression(s)
            let new_expr1 = Expression::something(self.allocator, data1);
            let new_expr2 = Expression::something_else(self.allocator, data2);
            
            // Build replacement
            let mut list = BumpVec::new_in(self.allocator);
            list.push(new_expr1);
            list.push(new_expr2);
            
            // Replace the expression
            *expr = Expression::block(self.allocator, None, list, expr_type);
        }
    }
}
```

## Example: The untee Pass

Here's a complete example that converts `local.tee` to `local.set` + `local.get`:

```rust
use crate::expression::{ExprRef, Expression, ExpressionKind};
use crate::module::Module;
use crate::pass::Pass;
use crate::visitor::Visitor;
use bumpalo::collections::Vec as BumpVec;

pub struct Untee;

impl Pass for Untee {
    fn name(&self) -> &str {
        "untee"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let allocator = module.allocator();
        
        for func in &mut module.functions {
            if let Some(body) = &mut func.body {
                let mut transformer = UnteeTransformer { allocator };
                transformer.visit(body);
            }
        }
    }
}

struct UnteeTransformer<'a> {
    allocator: &'a bumpalo::Bump,
}

impl<'a> Visitor<'a> for UnteeTransformer<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::LocalTee { index, value } = &expr.kind {
            let local_index = *index;
            let tee_value = *value;
            let tee_type = expr.type_;
            
            // Transform: (local.tee $x value) 
            //         => (block (local.set $x value) (local.get $x))
            let set = Expression::local_set(self.allocator, local_index, tee_value);
            let get = Expression::local_get(self.allocator, local_index, tee_type);
            
            let mut list = BumpVec::new_in(self.allocator);
            list.push(set);
            list.push(get);
            
            *expr = Expression::block(self.allocator, None, list, tee_type);
        }
    }
}
```

## Testing Passes

Always add comprehensive tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::expression::{Expression, ExpressionKind};
    use crate::module::Function;
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_my_pass_transforms_correctly() {
        let bump = Bump::new();
        
        // Create input expression
        let input = create_test_expression(&bump);
        
        // Create function and module
        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(input),
        );
        
        let mut module = Module::new(&bump);
        module.add_function(func);
        
        // Run pass
        let mut pass = MyPass;
        pass.run(&mut module);
        
        // Verify transformation
        let body = module.functions[0].body.as_ref().unwrap();
        assert!(matches!(body.kind, ExpressionKind::ExpectedPattern { .. }));
    }
}
```

## Best Practices

### 1. Preserve Types
Always maintain correct types when transforming:

```rust
let expr_type = expr.type_;
// ... transform ...
*expr = Expression::something(allocator, ..., expr_type);
```

### 2. Handle All Cases
Use pattern matching exhaustively:

```rust
match &expr.kind {
    ExpressionKind::Pattern1 { .. } => { /* transform */ },
    ExpressionKind::Pattern2 { .. } => { /* transform */ },
    _ => { /* leave unchanged */ }
}
```

### 3. Recurse Through Children
The Visitor automatically handles recursion, but you can control it:

```rust
fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
    // Pre-order work here
    
    // Transform this node
    do_transformation(expr);
    
    // Visitor will recurse to children automatically
}
```

### 4. Test Edge Cases
- Empty blocks
- Nested structures
- Type preservation
- No transformation cases

## Performance Tips

1. **Minimize Allocations**: Reuse BumpVec when possible
2. **Early Returns**: Skip work when pattern doesn't match
3. **Single Pass**: Try to do all work in one traversal
4. **Avoid Cloning**: Use references and copies of primitives

## Common Patterns

### Pattern 1: In-Place Simplification
Replace complex expression with simpler equivalent:

```rust
if is_identity_operation(expr) {
    *expr = extract_operand(expr);
}
```

### Pattern 2: Wrap in Block
Add control structure around expression:

```rust
let original = *expr;
let mut list = BumpVec::new_in(allocator);
list.push(setup_expr);
list.push(original);
*expr = Expression::block(allocator, None, list, expr.type_);
```

### Pattern 3: Unwrap Block
Remove unnecessary nesting:

```rust
if let ExpressionKind::Block { list, .. } = &expr.kind {
    if list.len() == 1 {
        *expr = list[0];
    }
}
```

## Integration Tests

Test passes in combination:

```rust
#[test]
fn test_pass_pipeline() {
    let mut module = create_test_module();
    
    let mut pass1 = Pass1;
    pass1.run(&mut module);
    
    let mut pass2 = Pass2;
    pass2.run(&mut module);
    
    // Verify final result
    assert_expected_output(&module);
}
```

## See Also

- `expression.rs` - Expression types and helpers
- `visitor.rs` - Visitor pattern implementation
- `module.rs` - Module structure
- `passes/untee.rs` - Complete example pass
- `tests/pass_pipeline.rs` - Integration test examples
