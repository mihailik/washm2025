pub fn parse_wast_to_wasm_bytes(wat_text: &str) -> Vec<u8> {
    // Parse with the canonical WAST parser; do not fallback to the `wat` crate.
    let lexer = wast::lexer::Lexer::new(wat_text);
    let buf = wast::parser::ParseBuffer::new_with_lexer(lexer);
    if let Ok(buf) = buf {
        if let Ok(mut wat_data) = wast::parser::parse::<wast::Wat>(&buf) {
            return wat_data.encode().expect("failed to encode module");
        }
    }

    // Do not fallback to the `wat` crate: we want canonical parsing via `wast` only.
    panic!("failed to parse with `wast` parser")
}
