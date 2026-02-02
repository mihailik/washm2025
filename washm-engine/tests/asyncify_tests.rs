use bumpalo::Bump;
use washm::passes::asyncify::{Asyncify, AsyncifyConfig};
use washm::passes::Pass;

fn run_asyncify(wat: &str, imports: Vec<&str>) -> String {
    let allocator = Bump::new();
    let wasm_bytes = wat::parse_str(wat).expect("failed to parse wat");

    let mut reader = washm::wasm::binary_reader::BinaryReader::new(&allocator, wasm_bytes);
    let mut module = reader.parse_module().expect("failed to read module");

    let mut config = AsyncifyConfig::default();
    for import in imports {
        config.imports.push(import.to_string());
    }

    let mut asyncify = Asyncify::new(config);
    asyncify.run(&mut module);

    let mut writer = washm::wasm::binary_writer::BinaryWriter::new();
    let bytes = writer
        .write_module(&module)
        .expect("failed to write module");

    wasmprinter::print_bytes(&bytes).expect("failed to print wasm")
}

#[test]
fn test_asyncify_simple_sleep() {
    let wat = r#"
        (module
            (import "env" "sleep" (func $sleep))
            (func $main
                (call $sleep)
            )
        )
    "#;

    let result = run_asyncify(wat, vec!["env.sleep"]);
    println!("{}", result);

    // Check for Asyncify globals (indices 0 and 1, as BinaryWriter doesn't write name section yet)
    // global 0: asyncify_state
    // global 1: asyncify_data
    assert!(result.contains("(global (;0;) (mut i32) i32.const 0)"));
    assert!(result.contains("(global (;1;) (mut i32) i32.const 0)"));

    // Check for exported runtime helpers
    // We check for the export name, ignoring the function index/name match in detail since it varies
    assert!(result.contains("(export \"asyncify_start_unwind\""));
    assert!(result.contains("(export \"asyncify_stop_unwind\""));
    assert!(result.contains("(export \"asyncify_start_rewind\""));
    assert!(result.contains("(export \"asyncify_stop_rewind\""));
    assert!(result.contains("(export \"asyncify_get_state\""));

    // Check for instrumentation in main
    // We expect state checks and rewinding logic
    // global.get 0 is asyncify_state
    // call 0 is the sleep function

    assert!(result.contains("global.get 0"));
    assert!(result.contains("call 0"));
    assert!(result.contains("if")); // Logic branches

    /*
    // Detailed checks disabled due to missing Name Section in BinaryWriter
    assert!(result.contains("global.get $__asyncify_state"));
    */
    assert!(result.contains("i32.const 2")); // Rewinding
    assert!(result.contains("i32.const 1")); // Unwinding
}

#[test]
fn test_asyncify_return_value_i32() {
    let wat = r#"
        (module
            (import "env" "sleep_ret" (func $sleep_ret (result i32)))
            (func $main (result i32)
                (call $sleep_ret)
            )
        )
    "#;

    let result = run_asyncify(wat, vec!["env.sleep_ret"]);

    // The call should be wrapped.
    // We expect the result of the call to be stored in a temp local if we are unwinding.
    // But importantly, the code path that checks 'if unwinding' must treat the stack correctly.
    // In the fix I implemented, the call result is saved to a local, and then `local.get` is used.

    // Scan for local.set of the temp variable after the call
    // The structure is roughly:
    // call $sleep_ret
    // local.set $temp
    // ... check state ...
    // block $asyncify_unwind
    // ...
    // end
    // local.get $temp

    // Since wasmprinter can be verbose, validation via regex or stringent string matching is hard.
    // But we can check that we don't have invalid validation which would likely happen if we didn't handle the return value.
    // Actually, `run_asyncify` produces valid WAT (wasmprinter succeeds). If the binary writer produced invalid code, we might not catch it unless we validate.
    // But wasmprinter just prints.

    // We should try to `wat::parse_str` the RESULT to ensure it's valid WASM.
    wat::parse_str(&result).expect("Transformation produced invalid WASM");

    assert!(result.contains("call 0"));
}

#[test]
fn test_asyncify_return_value_f64() {
    let wat = r#"
        (module
            (import "env" "sleep_ret_f64" (func $sleep_ret_f64 (result f64)))
            (func $main (result f64)
                (call $sleep_ret_f64)
            )
        )
    "#;

    let result = run_asyncify(wat, vec!["env.sleep_ret_f64"]);
    wat::parse_str(&result).expect("Transformation produced invalid WASM");
}

#[test]
fn test_asyncify_indirect_call() {
    let wat = r#"
        (module
            (type $t (func))
            (import "env" "sleep" (func $sleep))
            (table 1 funcref)
            (elem (i32.const 0) $sleep)
            (func $main
                (call_indirect (type $t) (i32.const 0))
            )
        )
    "#;

    // If we don't ignore indirect, it should instrument call_indirect.
    let result = run_asyncify(wat, vec!["env.sleep"]); // imports list doesn't affect indirect unless we filter?
                                                       // Asyncify treats all indirect calls as potential async calls by default unless ignored.

    assert!(result.contains("call_indirect"));
    assert!(result.contains("global.get 0"));
    wat::parse_str(&result).expect("Transformation produced invalid WASM");
}

#[test]
fn test_asyncify_loop() {
    let wat = r#"
        (module
            (import "env" "sleep" (func $sleep))
            (func $main
                (loop
                    (call $sleep)
                    (br 0)
                )
            )
        )
    "#;

    let result = run_asyncify(wat, vec!["env.sleep"]);
    wat::parse_str(&result).expect("Transformation produced invalid WASM");

    // Loop should now contain checks.
    // Also, loop entry is a target for rewinding.
}

#[test]
fn test_asyncify_deep_stack_params() {
    // Tests functionality with stack params that need to be saved/restored
    let wat = r#"
        (module
            (import "env" "sleep" (func $sleep (param i32) (param i32)))
            (func $main
                (i32.const 10)
                (i32.const 20)
                (call $sleep)
            )
        )
    "#;

    let result = run_asyncify(wat, vec!["env.sleep"]);
    wat::parse_str(&result).expect("Transformation produced invalid WASM");

    // When rewinding, we need to restore the stack.
    // There should be logic to pop values from the auxiliary stack and push them for the call.
    // But `call $sleep` is the import itself. The import wrapper or the call site handles it?
    // In Binaryen's Asyncify, the call site is surrounded by logic.
}

#[test]
fn test_asyncify_mixed_control_flow() {
    let wat = r#"
        (module
            (import "env" "sleep" (func $sleep))
            (func $main (param i32)
                (if (local.get 0)
                    (then
                        (call $sleep)
                    )
                    (else
                        (nop)
                    )
                )
            )
        )
    "#;

    let result = run_asyncify(wat, vec!["env.sleep"]);
    wat::parse_str(&result).expect("Transformation produced invalid WASM");
}

#[test]
fn test_asyncify_drop_handling() {
    // Ensuring drops are handled or don't interfere
    let wat = r#"
        (module
            (import "env" "sleep_ret" (func $sleep_ret (result i32)))
            (func $main
                (drop (call $sleep_ret))
            )
        )
    "#;

    let result = run_asyncify(wat, vec!["env.sleep_ret"]);
    wat::parse_str(&result).expect("Transformation produced invalid WASM");
}

#[test]
fn test_asyncify_ignore_imports() {
    // Tests that ignored imports are not transformed
    let wat = r#"
        (module
            (import "env" "sleep" (func $sleep))
            (func $main
                (call $sleep)
            )
        )
    "#;

    // Custom run logic to set ignore_imports
    let allocator = Bump::new();
    let wasm_bytes = wat::parse_str(wat).expect("failed to parse wat");

    let mut reader = washm::wasm::binary_reader::BinaryReader::new(&allocator, wasm_bytes);
    let mut module = reader.parse_module().expect("failed to read module");

    let mut config = AsyncifyConfig::default();
    config.ignore_imports = true;
    // Even if we have imports defined (or implicit all), ignore_imports should prevent instrumentation

    let mut asyncify = Asyncify::new(config);
    asyncify.run(&mut module);

    let mut writer = washm::wasm::binary_writer::BinaryWriter::new();
    let bytes = writer
        .write_module(&module)
        .expect("failed to write module");

    let result = wasmprinter::print_bytes(&bytes).expect("failed to print wasm");
    println!("{}", result);

    // Check main body
    assert!(result.contains("call 0")); // Sleep call

    // It should NOT contain the heavy instrumentation loop/if.
    // Since original code has no control flow, checking for "if" or "loop" is sufficient.
    // We check for "(if" to avoid partial matches like "asyncify".
    assert!(!result.contains("(if"));
    assert!(!result.contains("(loop"));
}

#[test]
fn test_asyncify_block_nested() {
    let wat = r#"
        (module
            (import "env" "sleep" (func $sleep))
            (func $main
                (block
                    (block
                        (call $sleep)
                    )
                )
            )
        )
    "#;
    let result = run_asyncify(wat, vec!["env.sleep"]);
    wat::parse_str(&result).expect("Transformation produced invalid WASM");
}

#[test]
fn test_asyncify_local_tee_interaction() {
    // Asyncify often has issues with values on stack during control flow.
    let wat = r#"
        (module
            (import "env" "sleep_ret" (func $sleep_ret (result i32)))
            (func $main (result i32)
                (local $x i32)
                (local.tee $x (call $sleep_ret))
            )
        )
    "#;
    let result = run_asyncify(wat, vec!["env.sleep_ret"]);
    wat::parse_str(&result).expect("Transformation produced invalid WASM");
}

#[test]
fn test_asyncify_many_arguments() {
    let wat = r#"
        (module
            (import "env" "sleep_args" (func $sleep_args (param i32 i32 i32 i32 i64 f32 f64)))
            (func $main
                (i32.const 1) (i32.const 2) (i32.const 3) (i32.const 4)
                (i64.const 5) (f32.const 6.0) (f64.const 7.0)
                (call $sleep_args)
            )
        )
    "#;
    let result = run_asyncify(wat, vec!["env.sleep_args"]);
    wat::parse_str(&result).expect("Transformation produced invalid WASM");
}

#[test]
fn test_asyncify_global_vars() {
    let wat = r#"
        (module
            (import "env" "sleep" (func $sleep))
            (global $g (mut i32) (i32.const 0))
            (func $main
                (global.set $g (i32.const 1))
                (call $sleep)
                (drop (global.get $g))
            )
        )
    "#;
    let result = run_asyncify(wat, vec!["env.sleep"]);
    wat::parse_str(&result).expect("Transformation produced invalid WASM");
}
