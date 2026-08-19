use chronosdb::engine::DurableTransactionalHeap;

#[test]
fn committed_insert_is_visible_after_restart() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    {
        let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

        let writer = db.begin().unwrap();

        db.insert(writer.id(), b"persistent".to_vec()).unwrap();

        db.commit(writer.id()).unwrap();

        db.sync().unwrap();
    }

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let reader = db.begin().unwrap();

    let rows = db.visible_scan(&reader).unwrap();

    assert_eq!(rows.len(), 1);

    assert_eq!(rows[0].1.payload(), b"persistent");
}

#[test]
fn aborted_insert_stays_invisible_after_restart() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    {
        let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

        let writer = db.begin().unwrap();

        db.insert(writer.id(), b"bad".to_vec()).unwrap();

        db.abort(writer.id()).unwrap();

        db.sync().unwrap();
    }

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let reader = db.begin().unwrap();

    assert!(db.visible_scan(&reader,).unwrap().is_empty());
}

#[test]
fn crashed_insert_is_aborted_after_restart() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    {
        let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

        let writer = db.begin().unwrap();

        db.insert(writer.id(), b"crashed".to_vec()).unwrap();

        db.sync().unwrap();

        // Simulated crash:
        // no commit/abort.
    }

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let reader = db.begin().unwrap();

    assert!(db.visible_scan(&reader,).unwrap().is_empty());
}

#[test]
fn committed_update_survives_restart() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    {
        let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

        let creator = db.begin().unwrap();

        let record = db.insert(creator.id(), b"old".to_vec()).unwrap();

        db.commit(creator.id()).unwrap();

        let updater = db.begin().unwrap();

        db.update(&updater, record, b"new".to_vec()).unwrap();

        db.commit(updater.id()).unwrap();

        db.sync().unwrap();
    }

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let reader = db.begin().unwrap();

    let rows = db.visible_scan(&reader).unwrap();

    assert_eq!(rows.len(), 1);

    assert_eq!(rows[0].1.payload(), b"new");
}

#[test]
fn aborted_update_restores_old_version_after_restart() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    {
        let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

        let creator = db.begin().unwrap();

        let record = db.insert(creator.id(), b"old".to_vec()).unwrap();

        db.commit(creator.id()).unwrap();

        let updater = db.begin().unwrap();

        db.update(&updater, record, b"bad".to_vec()).unwrap();

        db.abort(updater.id()).unwrap();

        db.sync().unwrap();
    }

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let reader = db.begin().unwrap();

    let rows = db.visible_scan(&reader).unwrap();

    assert_eq!(rows.len(), 1);

    assert_eq!(rows[0].1.payload(), b"old");
}

#[test]
fn committed_delete_survives_restart() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    {
        let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

        let creator = db.begin().unwrap();

        let record = db.insert(creator.id(), b"alive".to_vec()).unwrap();

        db.commit(creator.id()).unwrap();

        let deleter = db.begin().unwrap();

        db.delete(&deleter, record).unwrap();

        db.commit(deleter.id()).unwrap();

        db.sync().unwrap();
    }

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let reader = db.begin().unwrap();

    assert!(db.visible_scan(&reader,).unwrap().is_empty());
}

#[test]
fn transaction_ids_continue_across_database_restart() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    {
        let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

        let first = db.begin().unwrap();

        assert_eq!(first.id(), 1);

        db.commit(first.id()).unwrap();
    }

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let second = db.begin().unwrap();

    assert_eq!(second.id(), 2);
}

#[test]
fn committed_insert_recovers_without_heap_flush() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    {
        let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

        let writer = db.begin().unwrap();

        db.insert(writer.id(), b"redo-from-wal".to_vec()).unwrap();

        db.commit(writer.id()).unwrap();

        /*
         * Critical part:
         *
         * Do NOT call db.sync().
         *
         * The page mutation exists in WAL and the COMMIT
         * flush made that WAL durable, but the buffered
         * heap page itself has not been forced to disk.
         *
         * Dropping here simulates a process crash.
         */
    }

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let reader = db.begin().unwrap();

    let rows = db.visible_scan(&reader).unwrap();

    assert_eq!(rows.len(), 1);

    assert_eq!(rows[0].1.payload(), b"redo-from-wal");
}

#[test]
fn committed_update_recovers_without_heap_flush() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    let record;

    {
        let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

        let creator = db.begin().unwrap();

        record = db.insert(creator.id(), b"old".to_vec()).unwrap();

        db.commit(creator.id()).unwrap();

        /*
         * Persist the initial committed version so this
         * test isolates recovery of the UPDATE itself.
         */
        db.sync().unwrap();

        let updater = db.begin().unwrap();

        db.update(&updater, record, b"new".to_vec()).unwrap();

        db.commit(updater.id()).unwrap();

        // No sync after UPDATE.
    }

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let reader = db.begin().unwrap();

    let rows = db.visible_scan(&reader).unwrap();

    assert_eq!(rows.len(), 1);

    assert_eq!(rows[0].1.payload(), b"new");
}

#[test]
fn aborted_buffered_insert_stays_invisible_after_restart() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    {
        let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

        let writer = db.begin().unwrap();

        db.insert(writer.id(), b"aborted".to_vec()).unwrap();

        db.abort(writer.id()).unwrap();

        // No heap sync.
    }

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let reader = db.begin().unwrap();

    assert!(db.visible_scan(&reader,).unwrap().is_empty());
}

#[test]
fn repeated_recovery_is_idempotent_for_heap_mutations() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    {
        let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

        let writer = db.begin().unwrap();

        db.insert(writer.id(), b"once".to_vec()).unwrap();

        db.commit(writer.id()).unwrap();
    }

    {
        let _first_restart = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();
    }

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let reader = db.begin().unwrap();

    let rows = db.visible_scan(&reader).unwrap();

    assert_eq!(rows.len(), 1);

    assert_eq!(rows[0].1.payload(), b"once");
}
