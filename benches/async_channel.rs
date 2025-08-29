use criterion::*;
use std::time::Duration;

mod common;
use common::*;

async fn _async_channel_unbounded_async(tx_count: usize, rx_count: usize, msg_count: usize) {
    let (tx, rx) = async_channel::unbounded();
    let mut th_tx = Vec::new();
    let mut th_rx = Vec::new();
    let mut send_counter: usize = 0;
    let _send_counter = msg_count / tx_count;
    for _tx_i in 0..tx_count {
        send_counter += _send_counter;
        let _tx = tx.clone();
        th_tx.push(tokio::spawn(async move {
            for i in 0.._send_counter {
                if let Err(e) = _tx.send(i).await {
                    panic!("send error: {:?}", e);
                }
            }
        }));
    }
    drop(tx);
    let mut recv_counter = 0;
    for _ in 0..(rx_count - 1) {
        let _rx = rx.clone();
        th_rx.push(tokio::spawn(async move {
            let mut i = 0;
            loop {
                match _rx.recv().await {
                    Ok(_) => {
                        i += 1;
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
            i
        }));
    }
    loop {
        match rx.recv().await {
            Ok(_) => {
                recv_counter += 1;
            }
            Err(_) => {
                break;
            }
        }
    }
    for th in th_tx {
        let _ = th.await;
    }
    for th in th_rx {
        if let Ok(count) = th.await {
            recv_counter += count;
        }
    }
    assert_eq!(send_counter, recv_counter);
}

async fn _async_channel_bounded_async(
    bound: usize, tx_count: usize, rx_count: usize, msg_count: usize,
) {
    let (tx, rx) = async_channel::bounded(bound);
    let mut th_tx = Vec::new();
    let mut th_rx = Vec::new();
    let mut send_counter: usize = 0;
    let _send_counter = msg_count / tx_count;
    for _tx_i in 0..tx_count {
        send_counter += _send_counter;
        let _tx = tx.clone();
        th_tx.push(tokio::spawn(async move {
            for i in 0.._send_counter {
                if let Err(e) = _tx.send(i).await {
                    panic!("send error: {:?}", e);
                }
            }
        }));
    }
    drop(tx);
    let mut recv_counter = 0;
    for _ in 0..(rx_count - 1) {
        let _rx = rx.clone();
        th_rx.push(tokio::spawn(async move {
            let mut i = 0;
            loop {
                match _rx.recv().await {
                    Ok(_) => {
                        i += 1;
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
            i
        }));
    }
    loop {
        match rx.recv().await {
            Ok(_) => {
                recv_counter += 1;
            }
            Err(_) => {
                break;
            }
        }
    }
    for th in th_tx {
        let _ = th.await;
    }
    for th in th_rx {
        if let Ok(count) = th.await {
            recv_counter += count;
        }
    }
    assert_eq!(send_counter, recv_counter);
}

fn bench_async_channel_unbounded_async(c: &mut Criterion) {
    let mut group = c.benchmark_group("async_channel_unbounded_async");
    group.significance_level(0.1).sample_size(50);
    group.measurement_time(Duration::from_secs(20));
    for input in [(1, 1), (2, 1), (4, 1), (8, 1), (16, 1)] {
        let param = Concurrency { tx_count: input.0, rx_count: input.1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpsc unbounded", &param), &param, |b, i| {
            b.to_async(get_runtime())
                .iter(|| _async_channel_unbounded_async(i.tx_count, i.rx_count, ONE_MILLION))
        });
    }
    for input in [(2, 2), (4, 4), (8, 8), (16, 16)] {
        let param = Concurrency { tx_count: input.0, rx_count: input.1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpmc unbounded", &param), &param, |b, i| {
            b.to_async(get_runtime())
                .iter(|| _async_channel_unbounded_async(i.tx_count, i.rx_count, ONE_MILLION))
        });
    }
}

fn bench_async_channel_bounded_async(c: &mut Criterion) {
    let mut group = c.benchmark_group("async_channel_bounded_async");
    group.significance_level(0.1).sample_size(50);
    group.measurement_time(Duration::from_secs(20));
    for input in [(1, 1), (2, 1), (4, 1), (8, 1), (16, 1)] {
        let param = Concurrency { tx_count: input.0, rx_count: input.1 };
        group.throughput(Throughput::Elements(TEN_THOUSAND as u64));
        group.bench_with_input(BenchmarkId::new("mpsc bound 1", &param), &param, |b, i| {
            b.to_async(get_runtime())
                .iter(|| _async_channel_bounded_async(1, i.tx_count, i.rx_count, TEN_THOUSAND))
        });
    }

    for input in [(1, 1), (2, 1), (4, 1), (8, 1), (16, 1)] {
        let param = Concurrency { tx_count: input.0, rx_count: input.1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpsc bound 100", &param), &param, |b, i| {
            b.to_async(get_runtime())
                .iter(|| _async_channel_bounded_async(100, i.tx_count, i.rx_count, ONE_MILLION))
        });
    }
    for input in [(2, 2), (4, 4), (8, 8), (16, 16)] {
        let param = Concurrency { tx_count: input.0, rx_count: input.1 };
        group.throughput(Throughput::Elements(ONE_MILLION as u64));
        group.bench_with_input(BenchmarkId::new("mpmc bound 100", &param), &param, |b, i| {
            b.to_async(get_runtime())
                .iter(|| _async_channel_bounded_async(100, i.tx_count, i.rx_count, ONE_MILLION))
        });
    }
}

criterion_group!(benches, bench_async_channel_bounded_async, bench_async_channel_unbounded_async,);
criterion_main!(benches);
