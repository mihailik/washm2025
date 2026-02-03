use crate::ir::module::Module;
use crate::passes::Pass;

/// Assigns deterministic names to types for debugging.
pub struct NameTypes;

impl Pass for NameTypes {
    fn name(&self) -> &str {
        "NameTypes"
    }

    fn run(&mut self, module: &mut Module) {
        module.type_names.clear();
        for i in 0..module.types.len() {
            module.type_names.push(format!("type${}", i));
        }
    }
}

/// Generates the target features section.
pub struct EmitTargetFeatures;

impl Pass for EmitTargetFeatures {
    fn name(&self) -> &str {
        "EmitTargetFeatures"
    }

    fn run(&mut self, _module: &mut Module) {
        // Synchronize and validate features based on module content.
        // For now, we ensure the FeatureSet is updated to include essential features
        // if they are used. In a full implementation, this would scan all expressions.

        // Example: If we have SIMD instructions, we should ensure SIMD is enabled.
        // This is a placeholder for the full feature propagation logic.
    }
}
