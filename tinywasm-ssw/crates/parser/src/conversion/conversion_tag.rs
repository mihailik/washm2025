use crate::Result;
use alloc::boxed::Box;
use tinywasm_types::{TagType, FuncType};
use wasmparser::TagType as WasmTagType;

// Convert wasmparser::Tag entries into tinywasm TagType values using the module's function
// type list. A wasmparser Tag references a function type via `func_type_idx` (it is the
// function signature used as the tag payload type). We map that function type's params
// into our `TagType.params` representation.
pub(crate) fn convert_module_tags<T: IntoIterator<Item = wasmparser::Result<WasmTagType>>>(
    tags: T,
    func_types: &[FuncType],
) -> Result<Vec<TagType>> {
    let mut res = Vec::new();
    for tag in tags.into_iter() {
        let wasm_tag_ty = tag?;
        // wasmparser::TagType exposes `func_type_idx`.
        let idx = wasm_tag_ty.func_type_idx as usize;
        if idx >= func_types.len() {
            return Err(crate::ParseError::UnsupportedOperator(format!(
                "Tag references unknown type index {}",
                idx
            )));
        }

        // Clone the params from the referenced function type.
        let params = func_types[idx].params.clone();

        // Convert wasmparser types (already converted in func_types) into our TagType
        // storage: `params` is a boxed slice of ValType.
        res.push(TagType { params });
    }
    Ok(res)
}
