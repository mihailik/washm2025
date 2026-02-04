pub mod identify_booleans;
pub mod identify_if_else;
pub mod identify_loops;
pub mod identify_pointers;
pub mod name_inference;
pub mod recombine_expressions;

pub use identify_booleans::IdentifyBooleans;
pub use identify_if_else::IdentifyIfElse;
pub use identify_loops::IdentifyLoops;
pub use identify_pointers::IdentifyPointers;
pub use name_inference::NameInferencePass;
pub use recombine_expressions::ExpressionRecombination;
