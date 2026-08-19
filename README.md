# ChronosDB

A transactional storage engine written in Rust, built from first principles to
explore the internals behind durable database systems.

ChronosDB implements page-oriented storage, a buffer pool, MVCC transactions,
write-ahead logging, crash recovery, checkpoints, WAL compaction, and safe
version reclamation without relying on an existing database engine.

## Highlights

- Page-oriented persistent storage with slotted pages and stable record IDs
- Buffer pool with pinning, dirty-page tracking, and eviction
- Snapshot-based MVCC with versioned INSERT, UPDATE, and DELETE
- Write-write conflict detection between concurrent transactions
- Write-ahead logging with monotonically increasing LSNs
- WAL-before-data durability enforcement using page LSNs
- Idempotent REDO crash recovery
- Durable BEGIN / COMMIT / ABORT transaction recovery
- Checkpoint-aware restart and WAL-prefix compaction
- MVCC vacuum using the oldest active transaction as a safe horizon
- Deterministic crash/restart invariant testing
- Criterion and reproducible end-to-end performance benchmarks

## Architecture

    Application
         |
         v
    Transactional Engine
         |
         +------------------+
         |                  |
         v                  v
    Transaction Manager   MVCC Visibility
         |                  |
         +--------+---------+
                  |
                  v
              MVCC Heap
                  |
                  v
            Slotted Pages
                  |
                  v
             Buffer Pool
                  |
                  v
            Disk Manager

    Mutations
        |
        v
    Write-Ahead Log
        |
        +----------------+
        |                |
        v                v
    Checkpoints       REDO Recovery
        |
        v
    WAL Compaction

For the complete design, see `docs/ARCHITECTURE.md`.

## MVCC

Tuple versions contain transaction metadata:

    xmin = transaction that created the version
    xmax = transaction that deleted or replaced it

Readers operate against stable transaction snapshots.

An update therefore creates a new version rather than overwriting the old
tuple:

    Version A
    xmin = T1
    xmax = T2

          |
          v

    Version B
    xmin = T2
    xmax = none

This allows an older reader to continue observing Version A while a newer
transaction sees Version B.

ChronosDB also detects active write-write conflicts so two concurrent writers
cannot silently replace the same tuple.

## Durability

ChronosDB enforces the write-ahead logging invariant:

    WAL durable through page LSN
                |
                v
         data page may flush

Each page stores the LSN of the latest WAL operation reflected in that page.

During recovery:

    WAL LSN <= page LSN  -> skip
    WAL LSN >  page LSN  -> redo

This makes REDO idempotent and allows recovery to be executed repeatedly
without duplicating mutations.

Transaction lifecycle events are also WAL-backed:

    BEGIN
    COMMIT
    ABORT

Transactions that were active when the process crashed are recovered as
aborted.

## Checkpointing and WAL Compaction

A checkpoint establishes a durable recovery boundary containing the
transaction state required for restart.

Instead of retaining an indefinitely growing historical WAL:

    [ old history ][ checkpoint ][ required suffix ]

ChronosDB can compact it to:

    [ required suffix ]

while preserving logical LSN progression.

This substantially reduces the amount of historical work required during
restart.

## Performance

Measured locally in optimized Rust release mode:

| Workload | Result |
|---|---:|
| 1K sequential insert, Criterion | 22.6K records/s |
| 1K sequential insert, deterministic median | 23.9K records/s |
| 5K sequential insert, deterministic median | 11.0K records/s |
| 1K MVCC visible scan | 53.8K rows/s |
| 5K full-WAL recovery, median | 106.416 ms |
| 5K compacted recovery, median | 7.841 ms |

For the 5,000-transaction recovery workload, checkpoint-aware WAL compaction
reduced median recovery latency from 106.416 ms to 7.841 ms:

    92.3% lower median recovery latency
    ~13.0x median recovery speedup

These measurements characterize ChronosDB itself and are not intended as
performance comparisons with production databases.

Raw methodology and results are documented in `docs/BENCHMARKS.md`.

## Crash Testing

The recovery suite intentionally simulates process termination at important
transaction boundaries, including:

- after BEGIN
- after an uncommitted INSERT
- after COMMIT but before explicit data-page synchronization
- during an UPDATE
- after a committed UPDATE
- after a committed DELETE
- after checkpointing with additional WAL activity
- after vacuum
- across repeated restarts

The suite verifies invariants such as:

- committed writes survive restart
- incomplete transactions remain invisible
- transaction IDs remain monotonic
- REDO is idempotent
- checkpoint/WAL compaction preserves recoverability
- vacuum does not violate MVCC visibility

Crash tests are repeatedly executed in CI to help detect nondeterministic
recovery failures.

## Repository Structure

    src/
      engine/        transactional storage-engine APIs
      recovery/      WAL, REDO, checkpoints, transaction recovery
      storage/       pages, slotted storage, heap and buffer management
      transaction/   snapshots, visibility, transaction state and versions
      bin/            reproducible benchmark driver

    tests/            integration and crash-recovery tests
    benches/          Criterion benchmarks
    experiments/      benchmark measurements
    docs/             architecture and performance documentation

## Run

Run the complete test suite:

    cargo test

Run formatting and static analysis:

    cargo fmt --all -- --check

    cargo clippy \
      --all-targets \
      --all-features \
      -- \
      -D warnings

Run the crash-recovery suite:

    cargo test --test crash_recovery

Run Criterion benchmarks:

    cargo bench --bench engine

Run the reproducible benchmark driver:

    cargo run --release --bin benchmark

## Correctness Invariants

ChronosDB is designed around explicit storage-engine invariants:

1. WAL becomes durable before the corresponding dirty data page.
2. REDO can execute repeatedly without changing an already recovered state.
3. Transaction snapshots remain stable after concurrent commits.
4. A transaction never sees another transaction's uncommitted writes.
5. Committed mutations survive process restart.
6. Transactions interrupted before commit recover as aborted.
7. MVCC versions remain protected while an active snapshot may need them.
8. Transaction identifiers remain monotonic through recovery and compaction.
9. Checkpoint/WAL compaction does not change recovered database state.

## Scope

ChronosDB focuses on transactional storage-engine internals.

It deliberately does not currently implement a SQL parser, query optimizer,
secondary indexes, replication protocol, or distributed consensus.

The goal is to expose and test the mechanisms underneath those higher-level
database features rather than hide them behind an existing storage system.

## Documentation

- `docs/ARCHITECTURE.md` - storage-engine design and invariants
- `docs/BENCHMARKS.md` - benchmark methodology and measured results
