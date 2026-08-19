# ChronosDB Benchmarks

ChronosDB includes reproducible benchmarks for evaluating storage-engine
behavior and recovery scaling.

These measurements characterize ChronosDB itself. They are not intended as
direct performance comparisons with production database systems.

## Environment

Benchmarks were run locally in optimized Rust release mode.

Primary commands:

    cargo bench --bench engine
    cargo run --release --bin benchmark

Five independent deterministic benchmark runs were collected for recovery
measurements.

## Sequential Insert

Criterion measured the 1,000-record workload at:

    time:       44.295 ms
    throughput: 22.576K records/s

The deterministic benchmark produced these five 1,000-record results:

    26,411.94 records/s
    22,621.09 records/s
    27,632.26 records/s
    18,272.99 records/s
    23,888.32 records/s

Median:

    23,888 records/s
    ~= 23.9K records/s

For the 5,000-record workload, the five deterministic measurements were:

    10,900.29 records/s
    10,985.28 records/s
     7,786.97 records/s
    11,182.70 records/s
    11,082.27 records/s

Median:

    10,985 records/s
    ~= 11.0K records/s

The reduction in throughput at the larger workload reflects the current heap
insertion strategy and growing storage-management work. ChronosDB currently
prioritizes correctness and explicit storage-engine behavior over optimized
free-space or page-selection structures.

## MVCC Visible Scan

Criterion measured a visible scan across 1,000 tuples at:

    time:       18.592 ms
    throughput: 53.785K rows/s

The workload evaluates persisted tuples through MVCC visibility rules rather
than performing a raw byte scan.

## Recovery Benchmark

The main recovery experiment compares:

1. recovery from the full historical WAL
2. recovery after checkpointing and WAL-prefix compaction

The database state is equivalent in both cases.

### 100-Transaction Workload

Five runs:

| Run | Full WAL | Compacted WAL |
|---:|---:|---:|
| 1 | 9.156 ms | 4.314 ms |
| 2 | 9.871 ms | 5.386 ms |
| 3 | 7.402 ms | 3.263 ms |
| 4 | 10.817 ms | 5.385 ms |
| 5 | 9.080 ms | 5.298 ms |

At this size, fixed process, filesystem, and startup costs remain a large
fraction of total recovery latency.

### 1,000-Transaction Workload

Five runs:

| Run | Full WAL | Compacted WAL |
|---:|---:|---:|
| 1 | 40.188 ms | 5.617 ms |
| 2 | 26.402 ms | 5.778 ms |
| 3 | 39.323 ms | 5.658 ms |
| 4 | 37.658 ms | 5.467 ms |
| 5 | 36.720 ms | 5.554 ms |

Representative throughput ranges:

    full WAL:       ~25K-38K records/s
    compacted WAL: ~173K-183K records/s

Checkpoint-aware recovery increasingly avoids historical replay work as the
WAL grows.

### 5,000-Transaction Workload

Five paired measurements:

| Run | Full WAL | Compacted WAL | Reduction | Speedup |
|---:|---:|---:|---:|---:|
| 1 | 101.817 ms | 7.841 ms | 92.3% | 13.0x |
| 2 | 98.506 ms | 8.122 ms | 91.8% | 12.1x |
| 3 | 120.168 ms | 6.781 ms | 94.4% | 17.7x |
| 4 | 115.282 ms | 7.648 ms | 93.4% | 15.1x |
| 5 | 106.416 ms | 10.829 ms | 89.8% | 9.8x |

Median full-WAL recovery latency:

    106.416 ms

Median compacted recovery latency:

    7.841 ms

Median paired recovery reduction:

    92.3%

Median paired recovery speedup:

    13.0x

This is the primary performance result of the recovery subsystem.

As WAL history grows, full recovery requires increasing historical work.
Checkpoint metadata preserves the transaction state required for restart,
allowing ChronosDB to remove obsolete WAL prefixes and recover primarily from
the required suffix.

## Benchmark Summary

| Workload | Result |
|---|---:|
| 1K sequential insert, Criterion | 22.6K records/s |
| 1K sequential insert, deterministic median | 23.9K records/s |
| 5K sequential insert, deterministic median | 11.0K records/s |
| 1K MVCC visible scan | 53.8K rows/s |
| 5K full-WAL recovery, median | 106.416 ms |
| 5K compacted recovery, median | 7.841 ms |
| Recovery latency reduction | 92.3% |
| Recovery speedup | 13.0x |

## Reproducing Results

Compile the Criterion benchmark:

    cargo bench --bench engine --no-run

Run an individual Criterion workload:

    cargo bench --bench engine -- '^sequential_insert/1000$'

Run the deterministic benchmark:

    cargo run --release --bin benchmark

Capture results:

    cargo run --release --bin benchmark \
      | tee experiments/benchmarks/v1.csv

For repeated measurements:

    for i in {1..5}; do
      cargo run \
        --release \
        --bin benchmark \
        > "experiments/benchmarks/run_${i}.csv"
    done

## Interpretation

The benchmarks support three main observations.

First, the core engine can sustain tens of thousands of MVCC-aware storage
operations per second on the tested local workloads.

Second, MVCC-visible scans operate at roughly 54K rows/s on the 1K-row
Criterion workload.

Third, and most importantly, checkpoint-aware WAL compaction changes recovery
scaling substantially. On the 5,000-transaction benchmark, median restart
latency fell from 106.416 ms to 7.841 ms, a 92.3% reduction and approximately
13.0x speedup.

The recovery benchmark is therefore used as evidence for the architectural
value of checkpointing and WAL compaction rather than as a general-purpose
database performance claim.
