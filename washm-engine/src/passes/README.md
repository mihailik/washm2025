# Writing Optimization Passes

This guide explains how to write optimization passes for the WASHM IR.

## Overview

Passes transform WebAssembly IR to optimize code. The infrastructure provides:
- Generational ID-based arena memory allocation (`IdArena`)
- `IrBuilder` for easy expression creation
- Visitor pattern for tree traversal (ReadOnly and Mutable)
- Modular pass execution via `PassRunner`

## Basic Pass Structure

```rust
use crate::ir::module::Module;
use crate::passes::Pass;
use crate::ir::visitor::Visitor;

pub struct MyPass;

impl Pass for MyPass {
    fn name(&self) -> &str {
        "my-pass"
    }

    fn run(&mut self, module: &mut Module) {
        // Transform each function
        for (_, func) in &mut module.functions {
            if let Some(mut body) = func.body() {
                let mut transformer = MyTransformer { 
                    arena: &mut module.expressions 
                };
                transformer.visit(&mut body, &mut module.expressions);
                // Optionally update function body if the root changed
                func.set_body(body);
            }
        }
    }
}
```

## Creating New Expressions

Use `IrBuilder` with the module's expression arena:

```rust
let mut builder = IrBuilder::new(&mut module.expressions);

// Create a nop
let nop = builder.nop();

// Create a constant
let const_val = builder.const_(Literal::I32(42));

// Create a block
let list = vec![expr1, expr2];
let block = builder.block(None::<String>, list, Type::I32);
```

## Pattern: Transforming Expressions

The `Visitor` trait allows you to traverse the IR. To modify expressions in place, implement `Visitor` for your struct.

```rust
struct MyTransformer<'a> {
    arena: &'a mut IdArena<ExpressionMarker, Expression>,
}

impl<'a> Visitor for MyTransformer<'a> {
    fn visit(&mut self, expr_id: &mut ExpressionId, arena: &mut IdArena<ExpressionMarker, Expression>) {
        // First, visit children (Bottom-up transformation)
        self.super_visit(expr_id, arena);

        // Check if this is the pattern we want to transform
        match &arena[*expr_id].kind {
            ExpressionKind::Binary { op: BinaryOp::AddI32, left, right } => {
                // Example: x + 0 => x
                if let ExpressionKind::Const(Literal::I32(0)) = arena[*right].kind {
                    *expr_id = *left;
                } else if let ExpressionKind::Const(Literal::I32(0)) = arena[*left].kind {
                    *expr_id = *right;
                }
            }
            _ => {}
        }
    }
}
```

## Testing Passes

Always add comprehensive tests within your pass module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::module::Module;
    use crate::ir::expression::IrBuilder;

    #[test]
    fn test_my_pass_transforms_correctly() {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);
        
        let expr = builder.binary(
            BinaryOp::AddI32,
            builder.local_get(0, Type::I32),
            builder.const_(Literal::I32(0))
        );
        
        // Run pass
        let mut pass = MyPass;
        pass.run_on_expression(&mut expr, &mut module.expressions);
        
        // Assert result
        assert!(matches!(module.expressions[expr].kind, ExpressionKind::LocalGet { .. }));
    }
}
```
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
