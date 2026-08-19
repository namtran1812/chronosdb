use chronosdb::recovery::{LogManager, RecoveryManager};
use chronosdb::storage::{BufferPoolError, BufferPoolManager, DiskManager};

#[test]
fn logged_write_assigns_page_lsn() {
    let directory = tempfile::tempdir().unwrap();

    let db_path = directory.path().join("chronos.db");

    let wal_path = directory.path().join("chronos.wal");

    let disk = DiskManager::open(&db_path).unwrap();

    let wal = LogManager::open(&wal_path).unwrap();

    let mut buffer = BufferPoolManager::new_with_wal(disk, wal, 2);

    let page_id = buffer.new_page().unwrap();

    let lsn = buffer.logged_write(page_id, 64, b"chronos").unwrap();

    assert_eq!(buffer.page_lsn(page_id,), Some(lsn));

    assert_eq!(buffer.wal_durable_lsn(), None);
}

#[test]
fn explicit_page_flush_makes_wal_durable_first() {
    let directory = tempfile::tempdir().unwrap();

    let db_path = directory.path().join("chronos.db");

    let wal_path = directory.path().join("chronos.wal");

    let disk = DiskManager::open(&db_path).unwrap();

    let wal = LogManager::open(&wal_path).unwrap();

    let mut buffer = BufferPoolManager::new_with_wal(disk, wal, 2);

    let page_id = buffer.new_page().unwrap();

    let lsn = buffer.logged_write(page_id, 100, b"ordered").unwrap();

    assert_eq!(buffer.wal_durable_lsn(), None);

    buffer.flush_page(page_id).unwrap();

    assert!(
        buffer
            .wal_durable_lsn()
            .is_some_and(|durable| { durable >= lsn },)
    );

    drop(buffer);

    let mut disk = DiskManager::open(&db_path).unwrap();

    let page = disk.read_page(page_id).unwrap();

    assert_eq!(page.read(100, 7,).unwrap(), b"ordered");
}

#[test]
fn dirty_eviction_flushes_required_wal() {
    let directory = tempfile::tempdir().unwrap();

    let db_path = directory.path().join("chronos.db");

    let wal_path = directory.path().join("chronos.wal");

    let disk = DiskManager::open(&db_path).unwrap();

    let wal = LogManager::open(&wal_path).unwrap();

    let mut buffer = BufferPoolManager::new_with_wal(disk, wal, 1);

    let first = buffer.new_page().unwrap();

    let lsn = buffer.logged_write(first, 32, b"evicted").unwrap();

    buffer.unpin_page(first, true).unwrap();

    let second = buffer.new_page().unwrap();

    assert_ne!(first, second);

    assert!(
        buffer
            .wal_durable_lsn()
            .is_some_and(|durable| { durable >= lsn },)
    );

    drop(buffer);

    let mut disk = DiskManager::open(&db_path).unwrap();

    let page = disk.read_page(first).unwrap();

    assert_eq!(page.read(32, 7,).unwrap(), b"evicted");
}

#[test]
fn durable_wal_recovers_page_after_simulated_crash() {
    let directory = tempfile::tempdir().unwrap();

    let db_path = directory.path().join("chronos.db");

    let wal_path = directory.path().join("chronos.wal");

    {
        let disk = DiskManager::open(&db_path).unwrap();

        let wal = LogManager::open(&wal_path).unwrap();

        let mut buffer = BufferPoolManager::new_with_wal(disk, wal, 2);

        let page_id = buffer.new_page().unwrap();

        let lsn = buffer.logged_write(page_id, 128, b"redo-me").unwrap();

        buffer.flush_wal_through(lsn).unwrap();

        // Simulated crash:
        // drop without flushing
        // the dirty database page.
    }

    {
        let mut disk = DiskManager::open(&db_path).unwrap();

        let page = disk.read_page(0).unwrap();

        assert_eq!(page.read(128, 7,).unwrap(), &[0_u8; 7]);
    }

    {
        let mut wal = LogManager::open(&wal_path).unwrap();

        let mut disk = DiskManager::open(&db_path).unwrap();

        RecoveryManager::redo(&mut wal, &mut disk).unwrap();
    }

    let mut disk = DiskManager::open(&db_path).unwrap();

    let page = disk.read_page(0).unwrap();

    assert_eq!(page.read(128, 7,).unwrap(), b"redo-me");
}

#[test]
fn logged_write_requires_wal_configuration() {
    let directory = tempfile::tempdir().unwrap();

    let db_path = directory.path().join("chronos.db");

    let disk = DiskManager::open(&db_path).unwrap();

    let mut buffer = BufferPoolManager::new(disk, 2);

    let page_id = buffer.new_page().unwrap();

    assert_eq!(
        buffer.logged_write(page_id, 0, b"no-wal",),
        Err(BufferPoolError::WalNotConfigured)
    );
}
