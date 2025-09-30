# Hanging Issue Report

## Summary

A hanging issue has been discovered in crossfire when using a specific combination of sync sending and async receiving with certain parameters.

## Issue Details

**Affected Pattern:**
- Channel: `mpmc::bounded_async` with capacity **100**
- Sender: Sync (`MTx`) in `tokio::task::spawn_blocking`
- Receiver: Async (`MAsyncRx`) in `tokio::spawn`
- Message count: **1000**

**Symptoms:**
- Program hangs indefinitely
- No error messages or panics
- Typically hangs after processing some messages
- Discovered during benchmark: `sync_send_async_recv_crossfire/mixed/100`

## Reproduction

### Method 1: Run Tests (Recommended)
```bash
# This test includes timeout detection
cargo test test_hanging_issue --features tokio -- --nocapture
```

### Method 2: Run Benchmark (Will Hang)
```bash
# WARNING: This will hang indefinitely - use timeout
timeout 30s cargo bench --bench hanging_issue
```

## Code Pattern That Hangs

```rust
use crossfire::mpmc;

#[tokio::main]
async fn main() {
    // Create bounded async channel with capacity 100
    let (atx, arx) = mpmc::bounded_async::<i32>(100);
    let stx: crossfire::MTx<i32> = atx.into();
    
    // Sync sender in blocking task
    let sender_task = tokio::task::spawn_blocking(move || {
        for i in 0..1000 {
            stx.send(i).unwrap(); // May hang here
        }
    });
    
    // Async receiver in async task
    let receiver_task = tokio::spawn(async move {
        for _ in 0..1000 {
            arx.recv().await.unwrap(); // May hang here
        }
    });
    
    // This will hang indefinitely
    sender_task.await.unwrap();
    receiver_task.await.unwrap();
}
```

## Working Variations

The issue does NOT occur with:

1. **Different capacity**: Works with capacity 10, 1000
2. **Pure async**: `async send + async recv` works fine
3. **Pure sync**: `sync send + sync recv` works fine  
4. **Smaller message count**: Works with < 1000 messages
5. **Different pattern**: `async send + sync recv` works fine

## Environment

- **Crossfire version**: 2.x
- **Tokio version**: 1.x
- **Rust version**: 1.70+

## Investigation Notes

This issue was discovered during channel performance benchmarking where crossfire was compared against other channel implementations (flume, kanal, std::sync::mpsc). The same pattern works correctly with all other channel libraries.

The hanging appears to be specific to:
- The interaction between `MTx` (sync) and `MAsyncRx` (async)
- Buffer capacity of exactly 100
- High message throughput (1000 messages)

## Files Added for Reproduction

1. `src/tests/test_hanging_issue.rs` - Test cases with timeout detection
2. `benches/hanging_issue.rs` - Benchmark that reproduces the exact hanging scenario
3. `src/tests/mod.rs` - Updated to include the new test module

## Next Steps

1. Run the tests to confirm the issue exists
2. Investigate the interaction between `MTx` and `MAsyncRx` 
3. Check for potential deadlocks in the channel implementation
4. Consider if this is related to the specific buffer size (100)
5. Examine the waker/notification mechanism between sync and async sides
