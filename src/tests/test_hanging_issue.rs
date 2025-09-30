//! Test for hanging issue with sync send + async recv pattern
//!
//! This test reproduces a hanging issue discovered during benchmarking where
//! the combination of sync sending (MTx) and async receiving (MAsyncRx) with
//! specific parameters causes the program to hang indefinitely.
//!
//! Issue details:
//! - Channel: bounded_async with capacity 100
//! - Sender: sync (MTx) in spawn_blocking task
//! - Receiver: async (MAsyncRx) in spawn task  
//! - Load: 1000 messages
//!
//! This was discovered in benchmark: sync_send_async_recv_crossfire/mixed/100

use crate::mpmc;
use std::hint::black_box;
use std::time::Duration;

#[tokio::test]
async fn test_sync_send_async_recv_hanging_issue() {
    // This test reproduces the exact scenario that causes hanging
    // If this test hangs, it confirms the issue exists

    println!("Testing sync send + async recv pattern that may hang...");
    println!("Channel capacity: 100, Messages: 1000");

    // Create the exact same channel setup that causes hanging
    let (atx, arx) = mpmc::bounded_async::<i32>(100);
    let stx: crate::MTx<i32> = atx.into();

    // This is the exact pattern that causes hanging
    let sender_task = tokio::task::spawn_blocking(move || {
        for i in 0..1000 {
            match stx.send(black_box(i)) {
                Ok(_) => {
                    // Progress indicator
                    if i % 200 == 0 {
                        println!("Sync sent: {}", i);
                    }
                }
                Err(e) => {
                    panic!("Sync send failed at {}: {:?}", i, e);
                }
            }
        }
        println!("Sync sender completed");
    });

    let receiver_task = tokio::spawn(async move {
        for i in 0..1000 {
            match arx.recv().await {
                Ok(value) => {
                    black_box(value);
                    // Progress indicator
                    if i % 200 == 0 {
                        println!("Async received: {} (value: {})", i, value);
                    }
                }
                Err(e) => {
                    panic!("Async recv failed at {}: {:?}", i, e);
                }
            }
        }
        println!("Async receiver completed");
    });

    // Use timeout to detect hanging - if this times out, the issue exists
    let timeout_duration = Duration::from_secs(10);
    let result = tokio::time::timeout(timeout_duration, async {
        let sender_result = sender_task.await.expect("Sender task panicked");
        let receiver_result = receiver_task.await.expect("Receiver task panicked");
        (sender_result, receiver_result)
    })
    .await;

    match result {
        Ok(_) => {
            println!("✓ Test completed successfully - no hanging detected");
        }
        Err(_) => {
            panic!(
                "✗ TEST TIMED OUT - This confirms the hanging issue exists!\n\
                   The sync send + async recv pattern with capacity 100 causes a hang."
            );
        }
    }
}

#[tokio::test]
async fn test_sync_send_async_recv_smaller_capacity() {
    // Test with smaller capacity to see if issue is capacity-specific
    println!("Testing sync send + async recv with capacity 10...");

    let (atx, arx) = mpmc::bounded_async::<i32>(10);
    let stx: crate::MTx<i32> = atx.into();

    let sender_task = tokio::task::spawn_blocking(move || {
        for i in 0..100 {
            stx.send(black_box(i)).expect("Send failed");
        }
    });

    let receiver_task = tokio::spawn(async move {
        for _ in 0..100 {
            black_box(arx.recv().await.expect("Recv failed"));
        }
    });

    // This should complete quickly
    let timeout_duration = Duration::from_secs(5);
    let result = tokio::time::timeout(timeout_duration, async {
        sender_task.await.expect("Sender task panicked");
        receiver_task.await.expect("Receiver task panicked");
    })
    .await;

    assert!(result.is_ok(), "Test with capacity 10 should not hang");
    println!("✓ Capacity 10 test completed successfully");
}

#[tokio::test]
async fn test_async_send_async_recv_same_capacity() {
    // Test pure async send + async recv with same capacity to isolate the issue
    println!("Testing async send + async recv with capacity 100...");

    let (atx, arx) = mpmc::bounded_async::<i32>(100);

    let sender_task = tokio::spawn(async move {
        for i in 0..1000 {
            atx.send(black_box(i)).await.expect("Send failed");
        }
    });

    let receiver_task = tokio::spawn(async move {
        for _ in 0..1000 {
            black_box(arx.recv().await.expect("Recv failed"));
        }
    });

    // This should complete quickly
    let timeout_duration = Duration::from_secs(5);
    let result = tokio::time::timeout(timeout_duration, async {
        sender_task.await.expect("Sender task panicked");
        receiver_task.await.expect("Receiver task panicked");
    })
    .await;

    assert!(result.is_ok(), "Pure async send + async recv should not hang");
    println!("✓ Pure async test completed successfully");
}
