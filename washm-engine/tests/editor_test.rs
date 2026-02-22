use washm::ir::editor::ModuleEditor;
use washm::ir::expression::ExpressionKind;
use washm::ir::Literal;
use washm::ir::Module;
use washm::ir::Type;

#[test]
fn test_surgical_editor_dx() {
    let mut module = Module::new();

    // 1. Create a logger function
    let logger_id = {
        let mut builder = module.builder();
        let body = builder.nop();
        module.add_function(washm::ir::module::Function::new(
            "logger".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            body,
        ))
    };

    // 2. Create a main function
    let main_id = {
        let mut builder = module.builder();
        let body = builder.const_(Literal::I32(42));
        module.add_function(washm::ir::module::Function::new(
            "main".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            body,
        ))
    };

    // 3. Use the Surgical Editor API
    ModuleEditor::new(&mut module)
        .edit_function(main_id)
        .intercept_call(logger_id);

    // 4. Verify the transformation
    let (expressions, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
    let func = functions.get(main_id.cast()).unwrap();
    let body_id = func.body().unwrap();

    // Body should now be a block
    if let ExpressionKind::Block { list, .. } = &expressions[body_id].kind {
        assert_eq!(list.len(), 2);
        // First is the call to logger
        if let ExpressionKind::Call { target, .. } = &expressions[list[0]].kind {
            assert_eq!(*target, logger_id);
        } else {
            panic!("Expected call to logger at index 0");
        }
    } else {
        panic!("Main function body should have been wrapped in a block");
    }
}

#[test]
fn test_edit_each() {
    let mut module = Module::new();
    let main_id = {
        let mut builder = module.builder();
        let c1 = builder.const_(Literal::I32(1));
        let c2 = builder.const_(Literal::I32(2));
        let body = builder.binary(washm::ir::ops::BinaryOp::AddInt32, c1, c2, Type::I32);
        module.add_function(washm::ir::module::Function::new(
            "main".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            body,
        ))
    };

    // Use edit_each to replace all I32 constants with 100
    ModuleEditor::new(&mut module)
        .edit_function(main_id)
        .edit_each(|handle| {
            if let ExpressionKind::Const(Literal::I32(_)) = &handle.arena[*handle.current_id].kind {
                let mut builder = handle.builder();
                let new_id = builder.const_(Literal::I32(100));
                handle.replace_with(new_id);
            }
        });

    // Verification
    let (expressions, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
    let func = functions.get(main_id.cast()).unwrap();
    let body_id = func.body().unwrap();
    if let ExpressionKind::Binary { left, right, .. } = &expressions[body_id].kind {
        if let ExpressionKind::Const(Literal::I32(v)) = &expressions[*left].kind {
            assert_eq!(*v, 100);
        } else {
            panic!("Left operand should be 100");
        }
        if let ExpressionKind::Const(Literal::I32(v)) = &expressions[*right].kind {
            assert_eq!(*v, 100);
        } else {
            panic!("Right operand should be 100");
        }
    } else {
        panic!("Body should be a binary expression");
    }
}
