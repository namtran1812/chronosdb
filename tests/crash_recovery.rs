use chronosdb::engine::DurableTransactionalHeap;

fn paths(directory: &tempfile::TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        directory.path().join("table.heap"),
        directory.path().join("chronos.wal"),
    )
}

#[test]
fn crash_after_begin_aborts_transaction() {
    let directory = tempfile::tempdir().unwrap();

    let (heap, wal) = paths(&directory);

    let crashed_id;

    {
        let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

        crashed_id = db.begin().unwrap().id();

        /*
         * Simulated crash:
         * transaction was begun but never finished.
         */
    }

    let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

    use chronosdb::transaction::TransactionState;

    assert_eq!(
        db.transaction_state(crashed_id,),
        Some(TransactionState::Aborted)
    );

    let next = db.begin().unwrap();

    assert!(next.id() > crashed_id);
}

#[test]
fn crash_after_uncommitted_insert_hides_tuple() {
    let directory = tempfile::tempdir().unwrap();

    let (heap, wal) = paths(&directory);

    {
        let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

        let writer = db.begin().unwrap();

        db.insert(writer.id(), b"uncommitted".to_vec()).unwrap();

        /*
         * No commit.
         * No explicit heap sync.
         */
    }

    let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

    let reader = db.begin().unwrap();

    assert!(db.visible_scan(&reader,).unwrap().is_empty());
}

#[test]
fn crash_after_commit_recovers_unflushed_insert() {
    let directory = tempfile::tempdir().unwrap();

    let (heap, wal) = paths(&directory);

    {
        let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

        let writer = db.begin().unwrap();

        db.insert(writer.id(), b"durable".to_vec()).unwrap();

        db.commit(writer.id()).unwrap();

        /*
         * No db.sync().
         *
         * Commit made WAL durable.
         * REDO must restore the page.
         */
    }

    let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

    let reader = db.begin().unwrap();

    let rows = db.visible_scan(&reader).unwrap();

    assert_eq!(rows.len(), 1);

    assert_eq!(rows[0].1.payload(), b"durable");
}

#[test]
fn crash_after_committed_update_recovers_new_version() {
    let directory = tempfile::tempdir().unwrap();

    let (heap, wal) = paths(&directory);

    let record;

    {
        let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

        let creator = db.begin().unwrap();

        record = db.insert(creator.id(), b"old".to_vec()).unwrap();

        db.commit(creator.id()).unwrap();

        db.sync().unwrap();

        let updater = db.begin().unwrap();

        db.update(&updater, record, b"new".to_vec()).unwrap();

        db.commit(updater.id()).unwrap();

        /*
         * Crash before dirty-page flush.
         */
    }

    let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

    let reader = db.begin().unwrap();

    let rows = db.visible_scan(&reader).unwrap();

    assert_eq!(rows.len(), 1);

    assert_eq!(rows[0].1.payload(), b"new");
}

#[test]
fn crash_during_uncommitted_update_restores_old_visibility() {
    let directory = tempfile::tempdir().unwrap();

    let (heap, wal) = paths(&directory);

    let record;

    {
        let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

        let creator = db.begin().unwrap();

        record = db.insert(creator.id(), b"old".to_vec()).unwrap();

        db.commit(creator.id()).unwrap();

        db.sync().unwrap();

        let updater = db.begin().unwrap();

        db.update(&updater, record, b"uncommitted-new".to_vec())
            .unwrap();

        /*
         * Crash before commit/abort.
         */
    }

    let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

    let reader = db.begin().unwrap();

    let rows = db.visible_scan(&reader).unwrap();

    assert_eq!(rows.len(), 1);

    assert_eq!(rows[0].1.payload(), b"old");
}

#[test]
fn crash_after_committed_delete_keeps_tuple_hidden() {
    let directory = tempfile::tempdir().unwrap();

    let (heap, wal) = paths(&directory);

    let record;

    {
        let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

        let creator = db.begin().unwrap();

        record = db.insert(creator.id(), b"delete-me".to_vec()).unwrap();

        db.commit(creator.id()).unwrap();

        db.sync().unwrap();

        let deleter = db.begin().unwrap();

        db.delete(&deleter, record).unwrap();

        db.commit(deleter.id()).unwrap();

        /*
         * No final heap sync.
         */
    }

    let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

    let reader = db.begin().unwrap();

    assert!(db.visible_scan(&reader,).unwrap().is_empty());
}

#[test]
fn repeated_restart_is_idempotent() {
    let directory = tempfile::tempdir().unwrap();

    let (heap, wal) = paths(&directory);

    {
        let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

        for i in 0..20 {
            let tx = db.begin().unwrap();

            db.insert(tx.id(), format!("row-{i}").into_bytes()).unwrap();

            db.commit(tx.id()).unwrap();
        }
    }

    for _ in 0..5 {
        let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

        let reader = db.begin().unwrap();

        let rows = db.visible_scan(&reader).unwrap();

        assert_eq!(rows.len(), 20);
    }
}

#[test]
fn checkpoint_then_crash_recovers_wal_suffix() {
    let directory = tempfile::tempdir().unwrap();

    let (heap, wal) = paths(&directory);

    {
        let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

        let first = db.begin().unwrap();

        db.insert(first.id(), b"checkpointed".to_vec()).unwrap();

        db.commit(first.id()).unwrap();

        db.checkpoint_and_compact().unwrap();

        let second = db.begin().unwrap();

        db.insert(second.id(), b"suffix".to_vec()).unwrap();

        db.commit(second.id()).unwrap();

        /*
         * Crash with an unflushed WAL suffix.
         */
    }

    let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

    let reader = db.begin().unwrap();

    let mut rows: Vec<Vec<u8>> = db
        .visible_scan(&reader)
        .unwrap()
        .into_iter()
        .map(|(_, version)| version.payload().to_vec())
        .collect();

    rows.sort();

    assert_eq!(rows, vec![b"checkpointed".to_vec(), b"suffix".to_vec(),]);
}

#[test]
fn vacuum_then_crash_remains_recovered() {
    let directory = tempfile::tempdir().unwrap();

    let (heap, wal) = paths(&directory);

    {
        let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

        let tx = db.begin().unwrap();

        db.insert(tx.id(), b"garbage".to_vec()).unwrap();

        db.abort(tx.id()).unwrap();

        assert_eq!(db.vacuum().unwrap(), 1);

        /*
         * No explicit heap sync.
         * WAL-backed vacuum must recover.
         */
    }

    let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

    let reader = db.begin().unwrap();

    assert!(db.visible_scan(&reader,).unwrap().is_empty());
}

#[test]
fn transaction_ids_remain_monotonic_through_restarts_and_compaction() {
    let directory = tempfile::tempdir().unwrap();

    let (heap, wal) = paths(&directory);

    let highest_before;

    {
        let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

        let mut last = 0;

        for _ in 0..10 {
            let tx = db.begin().unwrap();

            last = tx.id();

            db.commit(tx.id()).unwrap();
        }

        highest_before = last;

        db.checkpoint_and_compact().unwrap();
    }

    let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

    let next = db.begin().unwrap();

    assert_eq!(next.id(), highest_before + 1);
}
