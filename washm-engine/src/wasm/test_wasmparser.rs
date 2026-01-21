#[cfg(test)]
mod tests {
    use wasmparser::*;
    #[test]
    fn test_types() {
        let _bt = BlockType::Empty;
        let gt = GlobalType {
            content_type: ValType::I32,
            mutable: true,
            shared: false,
        };
        let _ = gt;
    }
}
