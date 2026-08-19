use chronosdb::storage::{MvccPage, SlottedPage};
use chronosdb::transaction::{TransactionManager, TupleVersion};

#[test]
fn mvcc_tuple_survives_slotted_page_round_trip() {
    let mut page = MvccPage::new(3);

    let version = TupleVersion::new(9, b"persisted".to_vec());

    let record_id = page.insert_version(&version).unwrap();

    let bytes = Box::new(*page.slotted().as_bytes());

    let restored = MvccPage::from_slotted(3, SlottedPage::from_bytes(bytes));

    let decoded = restored.get_version(record_id.slot_id()).unwrap();

    assert_eq!(decoded, version);

    assert_eq!(record_id.page_id(), 3);
}

#[test]
fn visible_scan_hides_uncommitted_insert() {
    let mut manager = TransactionManager::new();

    let writer = manager.begin();

    let reader = manager.begin();

    let mut page = MvccPage::new(0);

    page.insert_version(&TupleVersion::new(writer.id(), b"draft".to_vec()))
        .unwrap();

    let visible = page
        .visible_versions(reader.snapshot(), reader.id(), |txid| manager.state(txid))
        .unwrap();

    assert!(visible.is_empty());
}

#[test]
fn visible_scan_returns_committed_insert() {
    let mut manager = TransactionManager::new();

    let writer = manager.begin();

    let mut page = MvccPage::new(0);

    page.insert_version(&TupleVersion::new(writer.id(), b"value".to_vec()))
        .unwrap();

    manager.commit(writer.id()).unwrap();

    let reader = manager.begin();

    let visible = page
        .visible_versions(reader.snapshot(), reader.id(), |txid| manager.state(txid))
        .unwrap();

    assert_eq!(visible.len(), 1);

    assert_eq!(visible[0].1.payload(), b"value");
}

#[test]
fn old_snapshot_sees_old_persistent_version() {
    let mut manager = TransactionManager::new();

    let creator = manager.begin();

    let mut page = MvccPage::new(0);

    let mut old = TupleVersion::new(creator.id(), b"old".to_vec());

    page.insert_version(&old).unwrap();

    manager.commit(creator.id()).unwrap();

    let old_reader = manager.begin();

    let updater = manager.begin();

    old.mark_deleted(updater.id());

    let mut rewritten = MvccPage::new(0);

    rewritten.insert_version(&old).unwrap();

    rewritten
        .insert_version(&TupleVersion::new(updater.id(), b"new".to_vec()))
        .unwrap();

    manager.commit(updater.id()).unwrap();

    let visible = rewritten
        .visible_versions(old_reader.snapshot(), old_reader.id(), |txid| {
            manager.state(txid)
        })
        .unwrap();

    assert_eq!(visible.len(), 1);

    assert_eq!(visible[0].1.payload(), b"old");
}

#[test]
fn new_snapshot_sees_new_persistent_version() {
    let mut manager = TransactionManager::new();

    let creator = manager.begin();

    let updater_id;

    let mut page = MvccPage::new(0);

    let mut old = TupleVersion::new(creator.id(), b"old".to_vec());

    manager.commit(creator.id()).unwrap();

    {
        let updater = manager.begin();

        updater_id = updater.id();

        old.mark_deleted(updater_id);

        page.insert_version(&old).unwrap();

        page.insert_version(&TupleVersion::new(updater_id, b"new".to_vec()))
            .unwrap();

        manager.commit(updater_id).unwrap();
    }

    let reader = manager.begin();

    let visible = page
        .visible_versions(reader.snapshot(), reader.id(), |txid| manager.state(txid))
        .unwrap();

    assert_eq!(visible.len(), 1);

    assert_eq!(visible[0].1.payload(), b"new");
}
