use chronosdb::PAGE_SIZE;
use chronosdb::recovery::{LogManager, RecoveryManager};
use chronosdb::storage::{BufferPoolManager, DiskManager, Page};

#[test]
fn page_lsn_survives_disk_round_trip() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.db");

    {
        let mut disk = DiskManager::open(&path).unwrap();

        let mut page = disk.allocate_page().unwrap();

        page.set_page_lsn(42);

        disk.write_page(&page).unwrap();

        disk.sync().unwrap();
    }

    let mut disk = DiskManager::open(&path).unwrap();

    let page = disk.read_page(0).unwrap();

    assert_eq!(page.page_lsn(), Some(42));
}

#[test]
fn new_page_has_no_lsn() {
    let page = Page::new(0);

    assert_eq!(page.page_lsn(), None);
}

#[test]
fn page_header_is_not_user_writable() {
    let mut page = Page::new(0);

    assert!(page.write(0, b"header",).is_err());

    assert!(page.write(8, b"header",).is_err());
}

#[test]
fn logged_page_lsn_survives_flush_and_reopen() {
    let directory = tempfile::tempdir().unwrap();

    let db_path = directory.path().join("chronos.db");

    let wal_path = directory.path().join("chronos.wal");

    let lsn;

    {
        let disk = DiskManager::open(&db_path).unwrap();

        let wal = LogManager::open(&wal_path).unwrap();

        let mut buffer = BufferPoolManager::new_with_wal(disk, wal, 2);

        let page_id = buffer.new_page().unwrap();

        lsn = buffer.logged_write(page_id, 64, b"persisted").unwrap();

        buffer.flush_page(page_id).unwrap();
    }

    let mut disk = DiskManager::open(&db_path).unwrap();

    let page = disk.read_page(0).unwrap();

    assert_eq!(page.page_lsn(), Some(lsn));

    assert_eq!(page.read(64, 9,).unwrap(), b"persisted");
}

#[test]
fn redo_skips_record_already_reflected_by_page_lsn() {
    let directory = tempfile::tempdir().unwrap();

    let db_path = directory.path().join("chronos.db");

    let wal_path = directory.path().join("chronos.wal");

    {
        let mut log = LogManager::open(&wal_path).unwrap();

        let lsn = log.append_page_write(0, 64, b"once").unwrap();

        log.flush().unwrap();

        let mut disk = DiskManager::open(&db_path).unwrap();

        let mut page = disk.allocate_page().unwrap();

        page.write(64, b"once").unwrap();

        page.set_page_lsn(lsn);

        disk.write_page(&page).unwrap();

        disk.sync().unwrap();
    }

    let mut log = LogManager::open(&wal_path).unwrap();

    let mut disk = DiskManager::open(&db_path).unwrap();

    let stats = RecoveryManager::redo(&mut log, &mut disk).unwrap();

    assert_eq!(stats.records_seen, 1);

    assert_eq!(stats.records_redone, 0);
}

#[test]
fn user_payload_still_has_expected_capacity() {
    let mut page = Page::new(0);

    let payload = vec![7_u8; PAGE_SIZE - 16];

    page.write(16, &payload).unwrap();

    assert_eq!(page.read(16, payload.len(),).unwrap(), payload);
}
