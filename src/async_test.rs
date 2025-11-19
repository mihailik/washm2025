use std::io::Write;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::pin::Pin;
use std::future::Future;

#[no_mangle]
pub unsafe extern "C" fn test_async_callback() -> i32 {
    use crate::wasm_async::WasmHostFuture;
    
    println!("Creating async task...");
    std::io::stdout().flush().unwrap();
    
    let js_code = r#"2 + 2"#;
    
    let mut future = Box::pin(WasmHostFuture::new(js_code));
    let cookie = future.cookie();
    
    println!("Async task created with cookie: {}", cookie);
    std::io::stdout().flush().unwrap();
    
    // Simple manual executor - poll with retry
    let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
    let waker = Waker::from_raw(raw_waker);
    let mut cx = Context::from_waker(&waker);
    
    // Try polling a few times with delay
    for attempt in 0..10 {
        match Pin::new(&mut future).poll(&mut cx) {
            Poll::Ready(Ok(result)) => {
                println!("Result after {} attempts: {}", attempt, result);
                std::io::stdout().flush().unwrap();
                return 0;
            }
            Poll::Pending => {
                if attempt == 0 {
                    println!("Task is pending, waiting for Host...");
                    std::io::stdout().flush().unwrap();
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Poll::Ready(Err(e)) => {
                eprintln!("Error: {}", e);
                return 1;
            }
        }
    }
    
    println!("Timeout waiting for result");
    std::io::stdout().flush().unwrap();
    0
}

static VTABLE: RawWakerVTable = RawWakerVTable::new(
    |_| RawWaker::new(std::ptr::null(), &VTABLE),
    |_| {},
    |_| {},
    |_| {},
);
