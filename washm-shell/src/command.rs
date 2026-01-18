use std::collections::HashMap;
use std::sync::Arc;

pub trait Command: Send + Sync {
    /// Static name used for dispatching commands like "ls" or "echo".
    fn name(&self) -> &'static str;

    /// Run a command with `args`; return numeric exit code.
    fn run(&self, args: &[String]) -> Result<i32, Box<dyn std::error::Error>>;
}

pub struct Dispatcher {
    cmds: HashMap<String, Arc<dyn Command>>,
}

impl Dispatcher {
    pub fn new() -> Self {
        Self {
            cmds: HashMap::new(),
        }
    }

    pub fn register<C: Command + 'static>(&mut self, cmd: C) {
        self.cmds.insert(cmd.name().to_string(), Arc::new(cmd));
    }

    pub fn dispatch(&self, name: &str, args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
        if let Some(cmd) = self.cmds.get(name) {
            cmd.run(args)
        } else {
            Err(format!("unknown command: {}", name).into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;

    impl Command for Echo {
        fn name(&self) -> &'static str {
            "echo"
        }

        fn run(&self, args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
            println!("{}", (args.join(" ")));
            Ok(0)
        }
    }

    #[test]
    fn echo_found() {
        let mut d = Dispatcher::new();
        d.register(Echo);
        let args: Vec<String> = vec!["hello".into(), "world".into()];
        let code = d.dispatch("echo", &args).unwrap();
        assert_eq!(code, 0);
    }
}
