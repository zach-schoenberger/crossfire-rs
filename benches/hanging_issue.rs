//! Benchmark that reproduces hanging issue
//!
//! This benchmark reproduces the exact hanging scenario discovered during
//! channel performance comparisons. The benchmark hangs indefinitely when run.
//!
//! Issue: sync_send_async_recv_crossfire/mixed/100
//! - Channel: bounded_async with capacity 100  
//! - Pattern: sync send (MTx) + async recv (MAsyncRx)
//! - Load: 1000 messages
//!
//! WARNING: This benchmark will hang! Use with timeout:
//! timeout 30s cargo bench --bench hanging_issue

use criterion::*;
use crossfire::*;
use std::hint::black_box;

#[allow(unused_imports)]
mod common;
use common::*;

fn bench_hanging_sync_send_async_recv(c: &mut Criterion) {
    detect_backoff_cfg();
    let mut group = c.benchmark_group("hanging_issue_sync_send_async_recv");
    group.sample_size(10); // Reduce sample size since this may hang

    // This is the exact configuration that causes hanging
    let capacity = 100;
    let message_count = 1000u64;

    group.throughput(Throughput::Elements(message_count));
    group.bench_with_input(
        BenchmarkId::new("mixed_sync_async", format!("cap_{}", capacity)),
        &capacity,
        |b, &capacity| {
            b.to_async(BenchExecutor()).iter(async || {
                // Create the exact same channel setup that hangs
                let (atx, arx) = mpmc::bounded_async::<i32>(capacity);
                let stx: crossfire::MTx<i32> = atx.into();

                // This is the exact pattern that causes hanging
                let sender_task = std::thread::spawn(move || {
                    for i in 0..message_count as i32 {
                        stx.send(black_box(i)).unwrap();
                    }
                });

                let receiver_task = async_spawn!(async move {
                    for _ in 0..message_count {
                        black_box(arx.recv().await.unwrap());
                    }
                });

                // WARNING: This will likely hang here
                sender_task.join().unwrap();
                async_join_result!(receiver_task);
            });
        },
    );
    group.finish();
}

fn bench_working_async_send_async_recv(c: &mut Criterion) {
    detect_backoff_cfg();
    let mut group = c.benchmark_group("working_comparison_async_async");

    // Same capacity and message count, but pure async - this should work
    let capacity = 100;
    let message_count = 1000u64;

    group.throughput(Throughput::Elements(message_count));
    group.bench_with_input(
        BenchmarkId::new("pure_async", format!("cap_{}", capacity)),
        &capacity,
        |b, &capacity| {
            b.to_async(BenchExecutor()).iter(async || {
                let (atx, arx) = mpmc::bounded_async::<i32>(capacity);

                let sender_task = async_spawn!(async move {
                    for i in 0..message_count as i32 {
                        atx.send(black_box(i)).await.unwrap();
                    }
                });

                let receiver_task = async_spawn!(async move {
                    for _ in 0..message_count {
                        black_box(arx.recv().await.unwrap());
                    }
                });

                // This should complete successfully
                async_join_result!(sender_task);
                async_join_result!(receiver_task);
            });
        },
    );
    group.finish();
}

fn bench_working_smaller_capacity(c: &mut Criterion) {
    detect_backoff_cfg();
    let mut group = c.benchmark_group("working_smaller_capacity");

    // Same pattern but smaller capacity - this should work
    let capacity = 10;
    let message_count = 1000u64;

    group.throughput(Throughput::Elements(message_count));
    group.bench_with_input(
        BenchmarkId::new("mixed_sync_async", format!("cap_{}", capacity)),
        &capacity,
        |b, &capacity| {
            b.to_async(BenchExecutor()).iter(async || {
                let (atx, arx) = mpmc::bounded_async::<i32>(capacity);
                let stx: crossfire::MTx<i32> = atx.into();

                let sender_task = std::thread::spawn(move || {
                    for i in 0..message_count as i32 {
                        stx.send(black_box(i)).unwrap();
                    }
                });

                let receiver_task = async_spawn!(async move {
                    for _ in 0..message_count {
                        black_box(arx.recv().await.unwrap());
                    }
                });

                // This should complete successfully
                sender_task.join().unwrap();
                async_join_result!(receiver_task);
            });
        },
    );
    group.finish();
}

criterion_group!(
    benches,
    bench_hanging_sync_send_async_recv,
    bench_working_async_send_async_recv,
    bench_working_smaller_capacity
);
criterion_main!(benches);
