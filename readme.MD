## Overview

Application instrumentation prototype, written in Rust. Optimizes for metric writes, stores data in TLV and occasionally
packs and sends snapshots to metric reader thread. 

## Benchmarks

```bash
# benchmark measurements using metrics crate
cargo run --release -- --tasks 1000 --mode ext-metrics

# single atomic increment
cargo run --release -- --tasks 1000 --mode atomic

# TLV-based metric engine
cargo run --release -- --tasks 1000 

# number of Tokio tasks can vary
cargo run --release -- --tasks 100000 
```