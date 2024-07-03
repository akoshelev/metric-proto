#![allow(dead_code)]
#![allow(unused_imports)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};
use clap::Parser;
use crossbeam::channel::unbounded;
use tokio::sync::Barrier;
use crate::atomic::ATOMIC_CTX;
use crate::metrics::{MetricKey, METRICS_CTX, Snapshot};

mod metrics;
mod atomic;
mod dimensions;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// what to do
    #[arg(long, default_value = "tlv")]
    mode: String,

    #[arg(long, default_value_t = 1000)]
    tasks: u64,

    #[arg(long, default_value_t = 100_000_000)]
    max_val: u64,

    #[arg(long)]
    threads: Option<u64>
}

async fn sleep_or_yield(elapsed: Duration) {
    const INTERVAL: Duration = Duration::from_nanos(10);
    if elapsed > INTERVAL {
        tokio::task::yield_now().await
    } else {
        tokio::time::sleep(INTERVAL - elapsed).await;
    }
}

fn main() {
    let args = Args::parse();
    let mut rt_builder = tokio::runtime::Builder::new_multi_thread();
    rt_builder.enable_all();

    if let Some(thread_count) = args.threads {
        rt_builder.worker_threads(thread_count as usize);
    }

    let (rt, tx, rx, atomic_cnt) = if args.mode == "atomic" {
        let counter = Arc::new(AtomicU64::default());
        rt_builder.on_thread_start({
            let counter = counter.clone();
            move || {
                let counter = Arc::clone(&counter);
                ATOMIC_CTX.with(move |m| m.connect(counter));
            }
        });
        (rt_builder.build().unwrap(), None, None, Some(counter))
    } else if args.mode == "tlv" {
        let (tx, rx) = unbounded();
        rt_builder.on_thread_start({
            let tx = tx.clone();
            move || {
                let tx = tx.clone();
                METRICS_CTX.with(move |m| {
                    m.connect(tx);
                });
            }
        }).on_thread_stop({
            let tx = tx.clone();
            move || {
                let snapshot = METRICS_CTX.with(|m| m.take_snapshot());
                if !snapshot.is_empty() {
                    let _ = tx.send(snapshot);
                }
            }
        }).on_thread_park({
            let tx = tx.clone();
            move || {
                let snapshot = METRICS_CTX.with(|m| m.take_snapshot());
                if !snapshot.is_empty() {
                    let _ = tx.send(snapshot);
                }
            }
        });

        (rt_builder.build().unwrap(), Some(tx), Some(rx), None)
    } else {
        panic!("unsupported mode: {}", args.mode);
    };
    drop(rt_builder);

    let start = Instant::now();
    for _ in 0..args.tasks {
        match args.mode.as_ref() {
            "atomic" => { rt.spawn(atomic::do_work_async()); },
            "tlv" => { rt.spawn(metrics::do_work_async()); },
            _ => unreachable!()
        }
    }
    println!("tasks started in {:?}", start.elapsed());


    let metric = if args.mode == "atomic" {
        let counter = atomic_cnt.unwrap();
        while counter.load(Ordering::Relaxed) < args.max_val {
            sleep(Duration::from_nanos(10));
            // counter.fetch_add(10_000, Ordering::Relaxed);
        }
        rt.shutdown_background();
        counter.load(Ordering::Relaxed)
    } else if args.mode == "tlv" {
        drop(tx);

        let mut snapshot = Snapshot::new();
        let rx = rx.unwrap();
        while let Ok(t) = rx.recv() {
            snapshot.merge(&t);
            if  snapshot.get(&MetricKey).unwrap_or_default().0 >= args.max_val {
                rt.shutdown_background();
                break;
            }
        }

        snapshot.get(&MetricKey).unwrap().0
    } else {
        unreachable!()
    };
    println!("mode: {}, metric: {:?}, elapsed {:?}", args.mode, metric, start.elapsed());
}
