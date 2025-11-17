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
        // check result
        let results = RESULTS.get_or_init(|| Mutex::new(HashMap::new()));
        if let Some(res) = results.lock().unwrap().remove(&cookie) {
            return Poll::Ready(Ok(res));
        }
        // register waker
        let conts = CONTINUATIONS.get_or_init(|| Mutex::new(HashMap::new()));
        conts.lock().unwrap().insert(cookie, cx.waker().clone());
        Poll::Pending
    }
}

// Note: only compiled for wasm32 targets when exported
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn guest_alloc(size: i32) -> i32 {
    init_maps();
    if size <= 0 { return 0; }
    let vec = vec![0u8; size as usize].into_boxed_slice();
    let ptr = vec.as_ptr() as usize as i32;
    // leak allocation so host may write into memory safely
    LEAKED_ALLOCS.get().unwrap().lock().unwrap().push(vec);
    ptr
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn _continue(ptr: i32) {
    // meta: 16 bytes
    if ptr == 0 { return; }
    unsafe {
        let meta = std::slice::from_raw_parts(ptr as *const u8, 16);
        let cookie = u32::from_le_bytes([meta[0], meta[1], meta[2], meta[3]]);
        // let status = u32::from_le_bytes([meta[4],meta[5],meta[6],meta[7]]);
        let result_size = u32::from_le_bytes([meta[8],meta[9],meta[10],meta[11]]) as usize;
        let result_ptr = u32::from_le_bytes([meta[12],meta[13],meta[14],meta[15]]) as usize;
        let result_slice = std::slice::from_raw_parts(result_ptr as *const u8, result_size);
        let res_string = String::from_utf8_lossy(result_slice).into_owned();
        let results = RESULTS.get_or_init(|| Mutex::new(HashMap::new()));
        results.lock().unwrap().insert(cookie, res_string);
        // wake
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
pub extern "C" fn guest_alloc(_size: i32) -> i32 { 0 }

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub extern "C" fn _continue(_ptr: i32) { }
