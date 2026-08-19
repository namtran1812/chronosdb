use chronosdb::engine::DurableTransactionalHeap;
use chronosdb::recovery::{Checkpoint, CheckpointManager, LogManager, RecoveryManager};
use chronosdb::storage::DiskManager;

#[test]
fn checkpoint_metadata_round_trips() {
    use std::collections::HashMap;

    use chronosdb::transaction::TransactionState;

    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.checkpoint");

    let manager = CheckpointManager::new(&path);

    assert_eq!(manager.load().unwrap(), None);

    let states = HashMap::from([
        (1, TransactionState::Committed),
        (2, TransactionState::Aborted),
    ]);

    manager
        .store(&Checkpoint::new(42, 3, states.clone()))
        .unwrap();

    let checkpoint = manager.load().unwrap().unwrap();

    assert_eq!(checkpoint.lsn(), 42);

    assert_eq!(checkpoint.next_transaction_id(), 3);

    assert_eq!(checkpoint.states(), &states);
}

#[test]
fn checkpoint_recovers_only_wal_suffix() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    {
        let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

        let first = db.begin().unwrap();

        db.insert(first.id(), b"before".to_vec()).unwrap();

        db.commit(first.id()).unwrap();

        let checkpoint_lsn = db.checkpoint().unwrap().unwrap();

        let second = db.begin().unwrap();

        db.insert(second.id(), b"after".to_vec()).unwrap();

        db.commit(second.id()).unwrap();

        assert!(checkpoint_lsn > 0);

        /*
         * Do not sync the second insert.
         * Its page image must be restored from
         * the WAL suffix after checkpoint.
         */
    }

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let reader = db.begin().unwrap();

    let mut payloads: Vec<Vec<u8>> = db
        .visible_scan(&reader)
        .unwrap()
        .into_iter()
        .map(|(_, version)| version.payload().to_vec())
        .collect();

    payloads.sort();

    assert_eq!(payloads, vec![b"after".to_vec(), b"before".to_vec(),]);
}

#[test]
fn redo_after_ignores_checkpoint_prefix() {
    let directory = tempfile::tempdir().unwrap();

    let db_path = directory.path().join("data.db");

    let wal_path = directory.path().join("chronos.wal");

    let checkpoint_lsn;

    {
        let mut disk = DiskManager::open(&db_path).unwrap();

        disk.allocate_page().unwrap();

        let mut wal = LogManager::open(&wal_path).unwrap();

        let first = wal.append_page_write(0, 32, b"first").unwrap();

        wal.flush_through(first).unwrap();

        RecoveryManager::redo(&mut wal, &mut disk).unwrap();

        checkpoint_lsn = first;

        wal.append_page_write(0, 64, b"second").unwrap();

        wal.flush().unwrap();
    }

    let mut wal = LogManager::open(&wal_path).unwrap();

    let mut disk = DiskManager::open(&db_path).unwrap();

    let stats = RecoveryManager::redo_after(&mut wal, &mut disk, checkpoint_lsn).unwrap();

    assert_eq!(stats.records_seen, 1);

    assert_eq!(stats.records_redone, 1);

    let page = disk.read_page(0).unwrap();

    assert_eq!(page.read(32, 5,).unwrap(), b"first");

    assert_eq!(page.read(64, 6,).unwrap(), b"second");
}

#[test]
fn repeated_restart_after_checkpoint_is_stable() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    {
        let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

        let writer = db.begin().unwrap();

        db.insert(writer.id(), b"stable".to_vec()).unwrap();

        db.commit(writer.id()).unwrap();

        db.checkpoint().unwrap();
    }

    for _ in 0..3 {
        let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

        let reader = db.begin().unwrap();

        let rows = db.visible_scan(&reader).unwrap();

        assert_eq!(rows.len(), 1);

        assert_eq!(rows[0].1.payload(), b"stable");
    }
}

#[test]
fn checkpoint_restores_transaction_states() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    {
        let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

        let committed = db.begin().unwrap();

        db.insert(committed.id(), b"keep".to_vec()).unwrap();

        db.commit(committed.id()).unwrap();

        let aborted = db.begin().unwrap();

        db.insert(aborted.id(), b"discard".to_vec()).unwrap();

        db.abort(aborted.id()).unwrap();

        db.checkpoint().unwrap();
    }

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let reader = db.begin().unwrap();

    /*
     * If checkpoint transaction state was not
     * restored correctly, xmin visibility would
     * incorrectly hide the committed tuple.
     */
    let rows = db.visible_scan(&reader).unwrap();

    assert_eq!(rows.len(), 1);

    assert_eq!(rows[0].1.payload(), b"keep");

    /*
     * IDs must continue from the checkpoint's
     * persisted next_transaction_id.
     */
    assert_eq!(reader.id(), 3);
}
