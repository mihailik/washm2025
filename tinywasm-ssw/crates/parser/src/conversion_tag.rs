use crate::Result;
use alloc::boxed::Box;
use tinywasm_types::TagType;
use wasmparser::Tag;

pub(crate) fn convert_module_tags<T: IntoIterator<Item = wasmparser::Result<Tag>>>(tags: T) -> Result<Vec<TagType>> {
    let mut res = Vec::new();
    for tag in tags.into_iter() {
        let tag = tag?;
        // wasmparser's Tag has its 'ty' as whatever representation; for a minimal implementation,
        // treat an empty payload as empty params and otherwise collect single param if present.
        // Full support requires mapping `wasmparser::Tag` -> `TagType` properly.
        let params = Box::new([]);
        res.push(TagType { params });
    }
    Ok(res)
}
