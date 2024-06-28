use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::sleep;
use std::time::Duration;
use clap::Parser;
use crossbeam::channel::unbounded;
use tokio::sync::Barrier;
use crate::metrics::{Counter, MetricKey, METRICS_CTX, MetricsContext, Snapshot};

mod metrics;
mod atomic;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// what to do
    #[arg(short, long, default_value = "r")]
    mode: String,

    #[arg(short, long, default_value_t = 1000)]
    tasks: u64,

    #[arg(short, long, default_value_t = 100_000_000)]
    iter: u64,

    #[arg(short, long, default_value_t = 5)]
    wait: u64
}

fn main() {
    let args = Args::parse();
    let (tx, rx) = unbounded();
    let mut rt_builder = tokio::runtime::Builder::new_multi_thread();
    rt_builder
        .enable_all();

    if args.mode != "atomic" {
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
                // tx.send(METRICS_CTX.with(|m| m.snapshot().clone())).unwrap();
                // METRICS_CTX.with(|m| m.disconnect());
            }
        });
    }
    let rt = rt_builder.build().unwrap();
    drop(rt_builder);

    for _ in 0..args.tasks {
        if args.mode == "atomic" {
            rt.spawn(atomic::do_work(args.iter));
        } else {
            rt.spawn(metrics::do_work(tx.clone(), args.iter));
        }
    }

    drop(tx);

    sleep(Duration::from_secs(args.wait));
    rt.shutdown_background();

    if args.mode == "atomic" {
        println!("atomic: {}", atomic::COUNTER.load(Ordering::Acquire));
    } else {
        let snapshot = Snapshot::new();
        // let counts = Arc::strong_count(&tx);
        // println!("counts: {counts}");
        // drop(tx);
        while let Ok(t) = rx.recv() {
            snapshot.merge(&t);
        }
        println!("metric: {}", snapshot.get(MetricKey).0);
    }
}
