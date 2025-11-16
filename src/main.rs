mod command;
mod uutils_bridge;
mod ion_bridge;

use command::Dispatcher;

struct Echo;

impl command::Command for Echo {
    fn name(&self) -> &'static str { "echo" }

    fn run(&self, args: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
        println!("{}", args.join(" "));
        Ok(0)
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    if args.len() <= 1 {
        println!("washm: usage: <command> [args...]");
        return;
    }

    let cmd = args.remove(1);

    let mut d = Dispatcher::new();
    d.register(Echo);

    // Integrate a stubbed uutils bridge. Real in-process calls to `coreutils-fork`
    // will be added later when the user is ready.
    if cmd == "uutils" {
        let exit = uutils_bridge::UutilsBridge::run("uutils", &args[1..]);
        std::process::exit(exit);
    }

    // `fish` integration removed — prefer `ion` for embedding and WASI port.
    
    // If user asks to run `ion`, run the embedded (static) ion library in-process.
    if cmd == "ion" {
        match ion_bridge::run(&args[1..]) {
            Ok(code) => std::process::exit(code),
            Err(err) => eprintln!("ion bridge error: {}", err),
        }
    }

    match d.dispatch(&cmd, &args[1..]) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    }
}