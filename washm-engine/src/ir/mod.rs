mod r#type;
pub use r#type::*;

mod literal;
pub use literal::*;

pub mod annotation;
pub mod expression;
pub mod module;
pub mod ops;
pub mod visitor;
pub mod wasm_features;

pub mod type_store;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod allocator_stress_test;
