/// Unified async sleep that yields control to host event loop via JavaScript setTimeout
/// This ensures single-threaded WASM never blocks on syscalls.

use crate::bridges::eval::{WasmHostFuture, eval_blocking};
use std::time::Duration;

const BRIDGE_JS: &str = include_str!("timer.js");

pub fn ensure_bridge() -> Result<(), String> {
    let js = format!("{}\n//# sourceURL=bridges/timer.js\n", BRIDGE_JS);
    eval_blocking(&js).map(|_| ())
}



/// Asynchronously sleep for the given duration by delegating to JS setTimeout.
/// This is non-blocking and yields control back to the host event loop.
pub async fn async_sleep(duration: Duration) -> Result<(), &'static str> {
    let _ = ensure_bridge();
    let millis = duration.as_millis() as u32;
    
    // Use JavaScript setTimeout to yield control to host event loop
    let js = format!(
        "washm_timer.sleep({})",
        millis
    );
    
    WasmHostFuture::new(&js).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_sleep_duration() {
        let start = std::time::Instant::now();
        async_sleep(Duration::from_millis(10)).await.unwrap();
        let elapsed = start.elapsed();
        // Allow some variance in timing
        assert!(elapsed.as_millis() >= 5);
    }
}
