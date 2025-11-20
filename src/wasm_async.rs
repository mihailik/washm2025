#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicU32, Ordering};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::path::Path;
use std::fs;

static COOKIE_COUNTER: AtomicU32 = AtomicU32::new(1);
static CONTINUATIONS: OnceLock<Mutex<HashMap<u32, Waker>>> = OnceLock::new();
static RESULTS: OnceLock<Mutex<HashMap<u32, String>>> = OnceLock::new();
static LEAKED_ALLOCS: OnceLock<Mutex<Vec<Box<[u8]>>>> = OnceLock::new();

fn init_maps() {
    CONTINUATIONS.get_or_init(|| Mutex::new(HashMap::new()));
    RESULTS.get_or_init(|| Mutex::new(HashMap::new()));
    LEAKED_ALLOCS.get_or_init(|| Mutex::new(Vec::new()));
}

pub fn next_cookie() -> u32 {
    COOKIE_COUNTER.fetch_add(1, Ordering::Relaxed)
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
        let _ = fs::write(req, js.as_bytes());
        WasmHostFuture { cookie }
    }

    pub fn cookie(&self) -> u32 { self.cookie }
}

impl Future for WasmHostFuture {
    type Output = Result<String, &'static str>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let cookie = self.cookie;
        let results = RESULTS.get_or_init(|| Mutex::new(HashMap::new()));
        if let Some(res) = results.lock().unwrap().remove(&cookie) {
            return Poll::Ready(Ok(res));
        }
        let conts = CONTINUATIONS.get_or_init(|| Mutex::new(HashMap::new()));
        conts.lock().unwrap().insert(cookie, cx.waker().clone());
        Poll::Pending
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
        let results = RESULTS.get_or_init(|| Mutex::new(HashMap::new()));
        results.lock().unwrap().insert(cookie, result_json);
        
        // Clean up the result file
        let _ = std::fs::remove_file(&result_file);
        
        // Wake the corresponding future
        if let Some(waker) = CONTINUATIONS.get().and_then(|m| m.lock().unwrap().remove(&cookie)) {
            waker.wake();
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn send_eval() -> i32 {
    // Quick PoC: write a JS request to /dev/host/eval/{cookie}.js and return 0 on success.
    let cookie = next_cookie();
    let path = Path::new("/dev/host/eval");
    if !path.exists() {
        let _ = fs::create_dir_all(path);
    }
    let req = path.join(format!("{}.js", cookie));
    let _ = fs::write(req, b"1+2");
    0
}



#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub extern "C" fn _continue(_cookie: u32) { }
