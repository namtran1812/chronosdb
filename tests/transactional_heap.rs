use std::fs;

use chronosdb::engine::{TransactionalHeap, TransactionalHeapError};
use chronosdb::transaction::TransactionState;

fn path(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "chronosdb-transactional-heap-{}-{}",
        name,
        std::process::id(),
    ));

    let _ = fs::remove_file(&path);

    path
}

#[test]
fn transaction_sees_own_insert() {
    let path = path("own-insert");

    let mut db = TransactionalHeap::open(&path).unwrap();

    let tx = db.begin();

    db.insert(tx.id(), b"alpha".to_vec()).unwrap();

    let rows = db.visible_scan(&tx).unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1.payload(), b"alpha",);
}

#[test]
fn other_transaction_does_not_see_uncommitted_insert() {
    let path = path("uncommitted");

    let mut db = TransactionalHeap::open(&path).unwrap();

    let writer = db.begin();

    db.insert(writer.id(), b"secret".to_vec()).unwrap();

    let reader = db.begin();

    let rows = db.visible_scan(&reader).unwrap();

    assert!(rows.is_empty());
}

#[test]
fn new_transaction_sees_committed_insert() {
    let path = path("committed");

    let mut db = TransactionalHeap::open(&path).unwrap();

    let writer = db.begin();

    db.insert(writer.id(), b"committed".to_vec()).unwrap();

    db.commit(writer.id()).unwrap();

    let reader = db.begin();

    let rows = db.visible_scan(&reader).unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1.payload(), b"committed",);
}

#[test]
fn old_snapshot_does_not_gain_visibility_after_commit() {
    let path = path("snapshot");

    let mut db = TransactionalHeap::open(&path).unwrap();

    let writer = db.begin();
    let reader = db.begin();

    db.insert(writer.id(), b"later".to_vec()).unwrap();

    db.commit(writer.id()).unwrap();

    let old_rows = db.visible_scan(&reader).unwrap();

    assert!(old_rows.is_empty());

    let new_reader = db.begin();

    let new_rows = db.visible_scan(&new_reader).unwrap();

    assert_eq!(new_rows.len(), 1);
    assert_eq!(new_rows[0].1.payload(), b"later",);
}

#[test]
fn aborted_insert_remains_invisible() {
    let path = path("abort");

    let mut db = TransactionalHeap::open(&path).unwrap();

    let writer = db.begin();

    db.insert(writer.id(), b"aborted".to_vec()).unwrap();

    db.abort(writer.id()).unwrap();

    assert_eq!(
        db.transaction_state(writer.id()),
        Some(TransactionState::Aborted),
    );

    let reader = db.begin();

    assert!(db.visible_scan(&reader).unwrap().is_empty());
}

#[test]
fn committed_rows_cross_page_boundaries() {
    let path = path("pages");

    let mut db = TransactionalHeap::open(&path).unwrap();

    let writer = db.begin();

    for index in 0..40 {
        db.insert(writer.id(), vec![index as u8; 512]).unwrap();
    }

    assert!(db.page_count() > 1);

    db.commit(writer.id()).unwrap();

    let reader = db.begin();

    let rows = db.visible_scan(&reader).unwrap();

    assert_eq!(rows.len(), 40);
}

#[test]
fn committed_data_survives_reopen() {
    let path = path("reopen");

    {
        let mut db = TransactionalHeap::open(&path).unwrap();

        let writer = db.begin();

        db.insert(writer.id(), b"persistent".to_vec()).unwrap();

        db.commit(writer.id()).unwrap();

        db.sync().unwrap();
    }

    let mut db = TransactionalHeap::open(&path).unwrap();

    /*
     * Transaction state is currently in-memory, so reopening
     * deliberately does not yet restore commit metadata.
     *
     * Physical persistence is checked through get() instead.
     */
    let version = db.get(chronosdb::storage::RecordId::new(0, 0)).unwrap();

    assert_eq!(version.payload(), b"persistent",);
}

#[test]
fn completed_transaction_cannot_write() {
    let path = path("finished-write");

    let mut db = TransactionalHeap::open(&path).unwrap();

    let tx = db.begin();

    db.commit(tx.id()).unwrap();

    let result = db.insert(tx.id(), b"invalid".to_vec());

    assert!(matches!(result, Err(TransactionalHeapError::NotActive),));
}

#[test]
fn committed_update_hides_old_version_from_new_reader() {
    let path = path("update");

    let mut db = TransactionalHeap::open(&path).unwrap();

    let creator = db.begin();

    let record = db.insert(creator.id(), b"old".to_vec()).unwrap();

    db.commit(creator.id()).unwrap();

    let old_reader = db.begin();

    let updater = db.begin();

    db.update(&updater, record, b"new".to_vec()).unwrap();

    db.commit(updater.id()).unwrap();

    let old_rows = db.visible_scan(&old_reader).unwrap();

    assert_eq!(old_rows.len(), 1);
    assert_eq!(old_rows[0].1.payload(), b"old");

    let new_reader = db.begin();

    let new_rows = db.visible_scan(&new_reader).unwrap();

    assert_eq!(new_rows.len(), 1);
    assert_eq!(new_rows[0].1.payload(), b"new");
}

#[test]
fn aborted_update_restores_old_visibility() {
    let path = path("abort-update");

    let mut db = TransactionalHeap::open(&path).unwrap();

    let creator = db.begin();

    let record = db.insert(creator.id(), b"old".to_vec()).unwrap();

    db.commit(creator.id()).unwrap();

    let updater = db.begin();

    db.update(&updater, record, b"bad".to_vec()).unwrap();

    db.abort(updater.id()).unwrap();

    let reader = db.begin();

    let rows = db.visible_scan(&reader).unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1.payload(), b"old");
}

#[test]
fn committed_delete_hides_record_from_new_reader() {
    let path = path("delete");

    let mut db = TransactionalHeap::open(&path).unwrap();

    let creator = db.begin();

    let record = db.insert(creator.id(), b"alive".to_vec()).unwrap();

    db.commit(creator.id()).unwrap();

    let old_reader = db.begin();

    let deleter = db.begin();

    db.delete(&deleter, record).unwrap();

    db.commit(deleter.id()).unwrap();

    let old_rows = db.visible_scan(&old_reader).unwrap();

    assert_eq!(old_rows.len(), 1);

    let new_reader = db.begin();

    assert!(db.visible_scan(&new_reader).unwrap().is_empty());
}

#[test]
fn aborted_delete_keeps_record_visible() {
    let path = path("abort-delete");

    let mut db = TransactionalHeap::open(&path).unwrap();

    let creator = db.begin();

    let record = db.insert(creator.id(), b"alive".to_vec()).unwrap();

    db.commit(creator.id()).unwrap();

    let deleter = db.begin();

    db.delete(&deleter, record).unwrap();

    db.abort(deleter.id()).unwrap();

    let reader = db.begin();

    let rows = db.visible_scan(&reader).unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1.payload(), b"alive");
}
