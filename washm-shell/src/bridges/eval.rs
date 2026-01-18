#![allow(dead_code)]

use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::{Context, Poll, Waker};

use std::cell::{Cell, RefCell};

static COOKIE_COUNTER: AtomicU32 = AtomicU32::new(1);

// Executor logic
use tokio::runtime::Runtime;
use tokio::task::LocalSet;

thread_local! {
    static CONTINUATIONS: RefCell<HashMap<u32, Waker>> = RefCell::new(HashMap::new());
    static RESULTS: RefCell<HashMap<u32, String>> = RefCell::new(HashMap::new());
    static LEAKED_ALLOCS: RefCell<Vec<Box<[u8]>>> = RefCell::new(Vec::new());

    static RT: Runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
    static LOCAL_SET: LocalSet = LocalSet::new();
    static IS_RUNNING: Cell<bool> = Cell::new(false);
}

pub fn spawn<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    LOCAL_SET.with(|ls| ls.spawn_local(future));
}

pub fn tick() {
    IS_RUNNING.with(|flag| flag.set(true));
    RT.with(|rt| {
        LOCAL_SET.with(|ls| {
            rt.block_on(ls.run_until(async {
                tokio::task::yield_now().await;
            }));
        });
    });
    IS_RUNNING.with(|flag| flag.set(false));
}

fn init_maps() {
    // No-op - thread_local handles initialization
}

pub fn next_cookie() -> u32 {
    COOKIE_COUNTER.fetch_add(1, Ordering::Relaxed)
}

const BRIDGE_JS: &str = include_str!("eval.js");

pub fn ensure_bridge() -> Result<(), String> {
    let js = format!("{}\n//# sourceURL=washm://src/bridges/eval.js\n", BRIDGE_JS);
    crate::bootstrap::eval_fire_and_forget(&js);
    Ok(())
}

pub struct WasmHostFuture {
    cookie: u32,
}

impl WasmHostFuture {
    pub fn new(js: &str) -> Self {
        init_maps();
        let cookie = next_cookie();
        let path = Path::new("/dev/host/eval");
        if !path.exists() {
            let _ = fs::create_dir_all(path);
        }
        let req = path.join(format!("{}.js", cookie));
        let _ = fs::write(&req, js.as_bytes());
        WasmHostFuture { cookie }
    }

    pub fn cookie(&self) -> u32 {
        self.cookie
    }
}

impl Future for WasmHostFuture {
    type Output = Result<String, &'static str>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let cookie = self.cookie;
        if let Some(res) = RESULTS.with(|m| m.borrow_mut().remove(&cookie)) {
            return Poll::Ready(Ok(res));
        }
        CONTINUATIONS.with(|m| m.borrow_mut().insert(cookie, cx.waker().clone()));
        Poll::Pending
    }
}

pub fn eval_blocking(js: &str) -> Result<String, String> {
    init_maps();
    let cookie = next_cookie();
    let path = Path::new("/dev/host/eval");
    if !path.exists() {
        let _ = fs::create_dir_all(path);
    }
    let req = path.join(format!("{}.js", cookie));
    if let Err(e) = fs::write(&req, js.as_bytes()) {
        return Err(e.to_string());
    }

    if let Some(res) = RESULTS.with(|m| m.borrow_mut().remove(&cookie)) {
        Ok(res)
    } else {
        // Include a snippet of the eval script to aid debugging. If the script is
        // longer than 7 lines include the first 4 and last 3 lines.
        let lines: Vec<&str> = js.lines().collect();
        let snippet = if lines.len() <= 7 {
            js.to_string()
        } else {
            let mut s = String::new();
            for l in &lines[0..4] {
                s.push_str(l);
                s.push('\n');
            }
            s.push_str("...\n");
            for l in &lines[lines.len().saturating_sub(3)..] {
                s.push_str(l);
                s.push('\n');
            }
            s
        };
        Err(format!(
            "ERROR error: I/O error: eval_blocking: result not ready immediately\n{}",
            snippet
        ))
    }
}

// Note: guest_alloc removed - host uses VFS-only communication

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn _continue(cookie: u32) {
    // Read result from VFS file
    let path = std::path::Path::new("/dev/host/eval");
    let result_file = path.join(format!("{}.json", cookie));

    if let Ok(result_json) = std::fs::read_to_string(&result_file) {
        // eprintln!("_continue: read {} bytes from {:?}", result_json.len(), result_file);
        RESULTS.with(|m| m.borrow_mut().insert(cookie, result_json));

        // Clean up the result file
        let _ = std::fs::remove_file(&result_file);

        // Wake the corresponding future
        if let Some(waker) = CONTINUATIONS.with(|m| m.borrow_mut().remove(&cookie)) {
            waker.wake();
        }

        // Tick the runtime to process the woken task, but only if we are not already running
        IS_RUNNING.with(|flag| {
            if !flag.get() {
                tick();
            }
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub extern "C" fn _continue(_cookie: u32) {}
