use crate::ir::module::Module;
use crate::passes::Pass;

/// Prints the module in WAT format to stdout.
pub struct Print;

impl Pass for Print {
    fn name(&self) -> &str {
        "Print"
    }

    fn run(&mut self, module: &mut Module) {
        match module.to_wat() {
            Ok(wat) => println!("{}", wat),
            Err(e) => log::error!("Error printing module: {}", e),
        }
    }
}
