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

#[test]
fn concurrent_updates_conflict() {
    use chronosdb::engine::DurableTransactionalHeapError;

    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let creator = db.begin().unwrap();

    let record = db.insert(creator.id(), b"base".to_vec()).unwrap();

    db.commit(creator.id()).unwrap();

    let first = db.begin().unwrap();

    let second = db.begin().unwrap();

    db.update(&first, record, b"first".to_vec()).unwrap();

    let result = db.update(&second, record, b"second".to_vec());

    assert!(matches!(
        result,
        Err(
            DurableTransactionalHeapError::
                WriteConflict(owner)
        ) if owner == first.id()
    ));
}

#[test]
fn update_delete_conflict() {
    use chronosdb::engine::DurableTransactionalHeapError;

    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let creator = db.begin().unwrap();

    let record = db.insert(creator.id(), b"value".to_vec()).unwrap();

    db.commit(creator.id()).unwrap();

    let updater = db.begin().unwrap();

    let deleter = db.begin().unwrap();

    db.update(&updater, record, b"changed".to_vec()).unwrap();

    let result = db.delete(&deleter, record);

    assert!(matches!(
        result,
        Err(
            DurableTransactionalHeapError::
                WriteConflict(owner)
        ) if owner == updater.id()
    ));
}

#[test]
fn delete_update_conflict() {
    use chronosdb::engine::DurableTransactionalHeapError;

    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let creator = db.begin().unwrap();

    let record = db.insert(creator.id(), b"value".to_vec()).unwrap();

    db.commit(creator.id()).unwrap();

    let deleter = db.begin().unwrap();

    let updater = db.begin().unwrap();

    db.delete(&deleter, record).unwrap();

    let result = db.update(&updater, record, b"changed".to_vec());

    assert!(matches!(
        result,
        Err(
            DurableTransactionalHeapError::
                WriteConflict(owner)
        ) if owner == deleter.id()
    ));
}

#[test]
fn concurrent_deletes_conflict() {
    use chronosdb::engine::DurableTransactionalHeapError;

    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let creator = db.begin().unwrap();

    let record = db.insert(creator.id(), b"value".to_vec()).unwrap();

    db.commit(creator.id()).unwrap();

    let first = db.begin().unwrap();

    let second = db.begin().unwrap();

    db.delete(&first, record).unwrap();

    let result = db.delete(&second, record);

    assert!(matches!(
        result,
        Err(
            DurableTransactionalHeapError::
                WriteConflict(owner)
        ) if owner == first.id()
    ));
}

#[test]
fn aborted_writer_releases_conflict() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let creator = db.begin().unwrap();

    let record = db.insert(creator.id(), b"base".to_vec()).unwrap();

    db.commit(creator.id()).unwrap();

    let failed_writer = db.begin().unwrap();

    db.update(&failed_writer, record, b"discarded".to_vec())
        .unwrap();

    db.abort(failed_writer.id()).unwrap();

    let retry = db.begin().unwrap();

    db.update(&retry, record, b"winner".to_vec()).unwrap();

    db.commit(retry.id()).unwrap();

    let reader = db.begin().unwrap();

    let rows = db.visible_scan(&reader).unwrap();

    assert_eq!(rows.len(), 1);

    assert_eq!(rows[0].1.payload(), b"winner");
}

#[test]
fn committed_writer_blocks_stale_snapshot_write() {
    use chronosdb::engine::DurableTransactionalHeapError;

    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let creator = db.begin().unwrap();

    let record = db.insert(creator.id(), b"base".to_vec()).unwrap();

    db.commit(creator.id()).unwrap();

    /*
     * Both writers take snapshots before either
     * performs the update.
     */
    let first = db.begin().unwrap();

    let stale = db.begin().unwrap();

    db.update(&first, record, b"first".to_vec()).unwrap();

    db.commit(first.id()).unwrap();

    let result = db.update(&stale, record, b"stale".to_vec());

    assert!(matches!(
        result,
        Err(
            DurableTransactionalHeapError::
                WriteConflict(owner)
        ) if owner == first.id()
    ));
}

#[test]
fn vacuum_reclaims_aborted_insert() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let writer = db.begin().unwrap();

    db.insert(writer.id(), b"dead".to_vec()).unwrap();

    db.abort(writer.id()).unwrap();

    assert_eq!(db.vacuum().unwrap(), 1);
}

#[test]
fn old_reader_blocks_vacuum_of_updated_version() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let creator = db.begin().unwrap();

    let record = db.insert(creator.id(), b"old".to_vec()).unwrap();

    db.commit(creator.id()).unwrap();

    /*
     * This snapshot must retain the old version.
     */
    let old_reader = db.begin().unwrap();

    let updater = db.begin().unwrap();

    db.update(&updater, record, b"new".to_vec()).unwrap();

    db.commit(updater.id()).unwrap();

    assert_eq!(db.vacuum().unwrap(), 0);

    let rows = db.visible_scan(&old_reader).unwrap();

    assert_eq!(rows.len(), 1);

    assert_eq!(rows[0].1.payload(), b"old");

    db.commit(old_reader.id()).unwrap();

    assert_eq!(db.vacuum().unwrap(), 1);
}

#[test]
fn vacuum_reclaims_committed_delete() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let creator = db.begin().unwrap();

    let record = db.insert(creator.id(), b"gone".to_vec()).unwrap();

    db.commit(creator.id()).unwrap();

    let deleter = db.begin().unwrap();

    db.delete(&deleter, record).unwrap();

    db.commit(deleter.id()).unwrap();

    assert_eq!(db.vacuum().unwrap(), 1);

    let reader = db.begin().unwrap();

    assert!(db.visible_scan(&reader,).unwrap().is_empty());
}

#[test]
fn vacuum_does_not_reclaim_aborted_delete() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let creator = db.begin().unwrap();

    let record = db.insert(creator.id(), b"keep".to_vec()).unwrap();

    db.commit(creator.id()).unwrap();

    let deleter = db.begin().unwrap();

    db.delete(&deleter, record).unwrap();

    db.abort(deleter.id()).unwrap();

    assert_eq!(db.vacuum().unwrap(), 0);

    let reader = db.begin().unwrap();

    let rows = db.visible_scan(&reader).unwrap();

    assert_eq!(rows.len(), 1);

    assert_eq!(rows[0].1.payload(), b"keep");
}

#[test]
fn vacuumed_slot_is_reused() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let failed = db.begin().unwrap();

    let dead = db.insert(failed.id(), b"dead".to_vec()).unwrap();

    db.abort(failed.id()).unwrap();

    assert_eq!(db.vacuum().unwrap(), 1);

    let writer = db.begin().unwrap();

    let live = db.insert(writer.id(), b"live".to_vec()).unwrap();

    /*
     * SlottedPage preferentially reuses
     * tombstoned SlotIds.
     */
    assert_eq!(live.page_id(), dead.page_id());

    assert_eq!(live.slot_id(), dead.slot_id());
}

#[test]
fn vacuum_survives_restart() {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    {
        let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

        let failed = db.begin().unwrap();

        db.insert(failed.id(), b"dead".to_vec()).unwrap();

        db.abort(failed.id()).unwrap();

        assert_eq!(db.vacuum().unwrap(), 1);

        /*
         * Vacuum page mutation is WAL-backed;
         * no explicit sync here.
         */
    }

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let reader = db.begin().unwrap();

    assert!(db.visible_scan(&reader,).unwrap().is_empty());
}
