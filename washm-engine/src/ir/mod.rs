mod r#type;
pub use r#type::*;

mod literal;
pub use literal::*;

pub mod annotation;
pub mod editor;
pub use editor::{FunctionEditor, ModuleEditor};
pub mod expression;
pub mod module;
pub use module::{DataId, ElementId, FuncId, GlobalId, LocalId, MemoryId, Module, TableId, TypeId};
pub mod ops;
pub mod visitor;
pub mod wasm_features;

pub mod type_store;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod allocator_stress_test;
