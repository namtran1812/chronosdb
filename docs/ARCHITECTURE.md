# ChronosDB Architecture

ChronosDB is a transactional storage engine written in Rust implementing core
database internals directly: page storage, buffer management, MVCC,
write-ahead logging, crash recovery, checkpoints, WAL compaction, and vacuum.

## System Architecture

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
        +----------+
        |          |
        v          v
    Checkpoint   REDO Recovery
        |
        v
    WAL Compaction

## Storage

ChronosDB uses fixed-size disk pages with stable page identifiers.

Records are stored using slotted pages. A record is addressed by:

    RecordId = (page_id, slot_id)

The slot directory separates logical record identity from physical byte
location, allowing page compaction without invalidating record identifiers.

The heap layer extends this abstraction across multiple pages and allocates new
pages when existing pages run out of space.

## Buffer Pool

The buffer pool caches pages in memory and tracks:

- page identity
- pin count
- dirty state
- replacement metadata

Pinned frames cannot be evicted.

Dirty pages are persisted before frame reuse.

For WAL-backed mutations, ChronosDB enforces WAL-before-data:

    WAL durable through page LSN
                |
                v
         page may be flushed

This prevents a data page from becoming durable before the log record required
to recover it.

## MVCC

Each tuple version stores:

    xmin = creating transaction
    xmax = deleting/replacing transaction

Transactions read through stable snapshots containing transaction visibility
information.

This means:

    T1 begins
    T2 writes
    T2 commits
    T1 reads

T1 retains its original snapshot and does not suddenly gain visibility of T2's
newly committed version.

Transactions also see their own uncommitted writes.

## Updates and Deletes

Updates create new versions rather than overwriting existing tuples.

    old version
    xmin = T1
    xmax = T2

    new version
    xmin = T2
    xmax = none

Older snapshots can continue observing the old version.

Deletes similarly set xmax instead of immediately reclaiming storage.

## Write Conflicts

ChronosDB detects concurrent write-write conflicts.

    T1 updates row
         |
         | T1 active
         v
    T2 updates row
         |
         v
    WriteConflict(T1)

If T1 aborts, its write ownership no longer blocks another transaction.

## Write-Ahead Log

WAL records receive monotonically increasing log sequence numbers.

The WAL contains page mutations and transaction lifecycle records:

    BEGIN
    COMMIT
    ABORT

Every persisted page stores its latest page LSN.

Recovery compares WAL and page LSNs:

    WAL LSN <= page LSN  -> skip
    WAL LSN >  page LSN  -> redo

This makes REDO idempotent.

## Crash Recovery

Opening a durable heap reconstructs transaction state and replays necessary WAL
records before normal processing resumes.

Transactions active when the process crashed are recovered as aborted.

Therefore:

- committed writes survive restart
- incomplete transactions remain invisible
- REDO can safely execute repeatedly
- transaction IDs continue monotonically

The crash suite covers failures around BEGIN, INSERT, UPDATE, DELETE, COMMIT,
checkpointing, vacuum, and repeated restart.

## Checkpoints

Checkpoints establish a durable recovery boundary.

Checkpoint metadata persists:

- recovery LSN
- transaction state
- transaction ID progression

Recovery can therefore process the WAL suffix instead of reconstructing state
from the entire historical log.

## WAL Compaction

After a checkpoint makes an older WAL prefix unnecessary, ChronosDB removes
that prefix.

    Before:
    [ obsolete WAL ][ checkpoint ][ required suffix ]

    After:
    [ required suffix ]

Logical LSN progression remains monotonic even though the physical WAL becomes
smaller.

## Vacuum

MVCC creates obsolete tuple versions.

ChronosDB calculates a reclamation horizon from the oldest active transaction.

A version is reclaimed only when no active snapshot can still require it.

Examples include:

- aborted inserts
- versions deleted before the safe horizon

Versions required by old snapshots remain protected.

Vacuum mutations are WAL-backed so reclamation remains correct after restart.

## Core Invariants

ChronosDB explicitly tests the following properties:

1. WAL-before-data ordering.
2. Idempotent REDO recovery.
3. Stable transaction snapshots.
4. No visibility of another transaction's uncommitted writes.
5. Committed mutations survive crashes.
6. Crashed uncommitted transactions become aborted.
7. MVCC versions are not reclaimed while visible to active snapshots.
8. Transaction IDs remain monotonic across recovery.
9. Checkpoint/WAL compaction preserves recoverability.

## Testing

Tests are layered across:

    Page
      |
    Slotted Page
      |
    Buffer Pool
      |
    WAL
      |
    MVCC
      |
    Heap
      |
    Transaction Engine
      |
    Recovery
      |
    Crash Invariants

The crash-recovery suite is executed repeatedly in CI to expose accidental
nondeterministic behavior.

## Scope

Implemented:

- persistent page storage
- slotted pages
- buffer pool
- stable record identifiers
- MVCC snapshots
- versioned INSERT / UPDATE / DELETE
- write-write conflict detection
- WAL
- page LSNs
- WAL-before-data
- REDO recovery
- transaction recovery
- checkpoints
- WAL compaction
- MVCC vacuum
- deterministic crash testing

Not currently implemented:

- SQL parser
- query optimizer
- secondary indexes
- distributed replication
- network protocol

ChronosDB focuses on transactional storage-engine internals rather than being a
full SQL database.
