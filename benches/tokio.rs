use criterion::*;
use std::time::Duration;

mod common;
use common::*;

async fn _tokio_bounded_mpsc(bound: usize, tx_count: usize, msg_count: usize) {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<usize>(bound);

    let _send_counter = msg_count / tx_count;
    for _tx_i in 0..tx_count {
        let _tx = tx.clone();
        async_spawn!(async move {
            for i in 0.._send_counter {
                let _ = _tx.send(i).await;
            }
        });
    }
    drop(tx);
    for _ in 0..(tx_count * _send_counter) {
        if let Some(_msg) = rx.recv().await {
            //    println!("recv {}", _msg);
        } else {
            panic!("recv error");
        }
    }
}

async fn _tokio_unbounded_mpsc(tx_count: usize, msg_count: usize) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<usize>();

    let _send_counter = msg_count / tx_count;
    for _tx_i in 0..tx_count {
        let _tx = tx.clone();
        async_spawn!(async move {
            for i in 0.._send_counter {
                let _ = _tx.send(i);
            }
        });
    }
    drop(tx);
    for _ in 0..(tx_count * _send_counter) {
        if let Some(_msg) = rx.recv().await {
            //    println!("recv {}", _msg);
        } else {
            panic!("recv error");
        }
    }
}

fn bench_tokio_bounded(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokio_bounded_100");
    group.significance_level(0.1).sample_size(50);
    group.measurement_time(Duration::from_secs(10));
    for input in n_1() {
        let param = Concurrency { tx_count: input, rx_count: 1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpsc", input), &param, |b, i| {
            b.to_async(BenchExecutor()).iter(|| _tokio_bounded_mpsc(100, i.tx_count, ONE_MILLION))
        });
    }
    group.finish();
}

fn bench_tokio_unbounded(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokio_unbounded");
    group.significance_level(0.1).sample_size(50);
    group.measurement_time(Duration::from_secs(10));
    for input in n_1() {
        let param = Concurrency { tx_count: input, rx_count: 1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpsc", input), &param, |b, i| {
            b.to_async(BenchExecutor()).iter(|| _tokio_unbounded_mpsc(i.tx_count, ONE_MILLION))
        });
    }
    group.finish();
}

criterion_group!(benches, bench_tokio_bounded, bench_tokio_unbounded);
criterion_main!(benches);
