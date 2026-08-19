use chronosdb::storage::{HeapFile, HeapFileError};
use chronosdb::transaction::{TransactionManager, TupleVersion};

#[test]
fn heap_file_starts_empty() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("table.heap");

    let heap = HeapFile::open(&path).unwrap();

    assert_eq!(heap.page_count(), 0);
}

#[test]
fn first_insert_allocates_page() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("table.heap");

    let mut heap = HeapFile::open(&path).unwrap();

    let record = heap
        .insert_version(&TupleVersion::new(1, b"first".to_vec()))
        .unwrap();

    assert_eq!(record.page_id(), 0);

    assert_eq!(record.slot_id(), 0);

    assert_eq!(heap.page_count(), 1);
}

#[test]
fn heap_record_survives_reopen() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("table.heap");

    let record_id;

    {
        let mut heap = HeapFile::open(&path).unwrap();

        record_id = heap
            .insert_version(&TupleVersion::new(7, b"persistent".to_vec()))
            .unwrap();

        heap.sync().unwrap();
    }

    let mut heap = HeapFile::open(&path).unwrap();

    let version = heap.get_version(record_id).unwrap();

    assert_eq!(version.xmin(), 7);

    assert_eq!(version.payload(), b"persistent");
}

#[test]
fn inserts_roll_over_to_multiple_pages() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("table.heap");

    let mut heap = HeapFile::open(&path).unwrap();

    let payload = vec![9_u8; 900];

    let mut records = Vec::new();

    for txid in 1..=20 {
        records.push(
            heap.insert_version(&TupleVersion::new(txid, payload.clone()))
                .unwrap(),
        );
    }

    assert!(heap.page_count() > 1);

    assert!(records.iter().any(|record| { record.page_id() > 0 },));
}

#[test]
fn record_ids_are_stable_across_pages() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("table.heap");

    let mut heap = HeapFile::open(&path).unwrap();

    let payload = vec![3_u8; 1000];

    let mut records = Vec::new();

    for txid in 1..=10 {
        records.push(
            heap.insert_version(&TupleVersion::new(txid, payload.clone()))
                .unwrap(),
        );
    }

    for (index, record_id) in records.iter().enumerate() {
        let version = heap.get_version(*record_id).unwrap();

        assert_eq!(version.xmin(), (index + 1) as u64);
    }
}

#[test]
fn visible_scan_crosses_page_boundaries() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("table.heap");

    let mut manager = TransactionManager::new();

    let mut heap = HeapFile::open(&path).unwrap();

    let payload = vec![5_u8; 900];

    for _ in 0..12 {
        let writer = manager.begin();

        heap.insert_version(&TupleVersion::new(writer.id(), payload.clone()))
            .unwrap();

        manager.commit(writer.id()).unwrap();
    }

    assert!(heap.page_count() > 1);

    let reader = manager.begin();

    let visible = heap
        .visible_scan(reader.snapshot(), reader.id(), |txid| manager.state(txid))
        .unwrap();

    assert_eq!(visible.len(), 12);
}

#[test]
fn visible_scan_filters_uncommitted_rows_across_pages() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("table.heap");

    let mut manager = TransactionManager::new();

    let mut heap = HeapFile::open(&path).unwrap();

    let payload = vec![8_u8; 900];

    for _ in 0..8 {
        let writer = manager.begin();

        heap.insert_version(&TupleVersion::new(writer.id(), payload.clone()))
            .unwrap();

        manager.commit(writer.id()).unwrap();
    }

    let uncommitted = manager.begin();

    heap.insert_version(&TupleVersion::new(uncommitted.id(), payload))
        .unwrap();

    let reader = manager.begin();

    let visible = heap
        .visible_scan(reader.snapshot(), reader.id(), |txid| manager.state(txid))
        .unwrap();

    assert_eq!(visible.len(), 8);
}

#[test]
fn oversized_tuple_is_rejected() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("table.heap");

    let mut heap = HeapFile::open(&path).unwrap();

    let huge = TupleVersion::new(1, vec![0_u8; 10_000]);

    assert!(matches!(
        heap.insert_version(&huge,),
        Err(HeapFileError::TupleTooLarge)
    ));
}
