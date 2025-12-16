/// Unified async sleep that yields control to host event loop via JavaScript setTimeout
/// This ensures single-threaded WASM never blocks on syscalls.

use crate::wasm_async::WasmHostFuture;
use std::time::Duration;

/// Asynchronously sleep for the given duration by delegating to JS setTimeout.
/// This is non-blocking and yields control back to the host event loop.
pub async fn async_sleep(duration: Duration) -> Result<(), &'static str> {
    let millis = duration.as_millis() as u32;
    
    // Use JavaScript setTimeout to yield control to host event loop
    let js = format!(
        "new Promise(resolve => setTimeout(() => resolve('done'), {}))",
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
