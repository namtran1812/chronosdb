use std::fs::OpenOptions;
use std::io::Write;

use chronosdb::recovery::{LogManager, RecoveryManager};
use chronosdb::storage::DiskManager;

#[test]
fn wal_assigns_monotonic_lsns() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.wal");

    let mut log = LogManager::open(&path).unwrap();

    let first = log.append_page_write(0, 10, b"alpha").unwrap();

    let second = log.append_page_write(0, 20, b"beta").unwrap();

    assert_eq!(first, 0);
    assert_eq!(second, 1);
    assert_eq!(log.next_lsn(), 2);
}

#[test]
fn flush_advances_durable_lsn() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.wal");

    let mut log = LogManager::open(&path).unwrap();

    let lsn = log.append_page_write(0, 0, b"chronos").unwrap();

    assert_eq!(log.durable_lsn(), None);

    log.flush().unwrap();

    assert_eq!(log.durable_lsn(), Some(lsn));
}

#[test]
fn wal_survives_reopen() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.wal");

    {
        let mut log = LogManager::open(&path).unwrap();

        log.append_page_write(4, 100, b"payload").unwrap();

        log.flush().unwrap();
    }

    let mut log = LogManager::open(&path).unwrap();

    let records = log.records().unwrap();

    assert_eq!(records.len(), 1);

    assert_eq!(records[0].page_id, 4);

    assert_eq!(records[0].payload, b"payload");

    assert_eq!(log.next_lsn(), 1);
}

#[test]
fn redo_recovers_unflushed_page() {
    let directory = tempfile::tempdir().unwrap();

    let wal_path = directory.path().join("chronos.wal");

    let db_path = directory.path().join("chronos.db");

    {
        let mut log = LogManager::open(&wal_path).unwrap();

        log.append_page_write(0, 64, b"recovered").unwrap();

        log.flush().unwrap();
    }

    {
        let mut log = LogManager::open(&wal_path).unwrap();

        let mut disk = DiskManager::open(&db_path).unwrap();

        let stats = RecoveryManager::redo(&mut log, &mut disk).unwrap();

        assert_eq!(stats.records_seen, 1);

        assert_eq!(stats.records_redone, 1);
    }

    let mut disk = DiskManager::open(&db_path).unwrap();

    let page = disk.read_page(0).unwrap();

    assert_eq!(page.read(64, 9,).unwrap(), b"recovered");
}

#[test]
fn redo_is_idempotent() {
    let directory = tempfile::tempdir().unwrap();

    let wal_path = directory.path().join("chronos.wal");

    let db_path = directory.path().join("chronos.db");

    {
        let mut log = LogManager::open(&wal_path).unwrap();

        log.append_page_write(0, 32, b"stable").unwrap();

        log.flush().unwrap();
    }

    for _ in 0..2 {
        let mut log = LogManager::open(&wal_path).unwrap();

        let mut disk = DiskManager::open(&db_path).unwrap();

        RecoveryManager::redo(&mut log, &mut disk).unwrap();
    }

    let mut disk = DiskManager::open(&db_path).unwrap();

    let page = disk.read_page(0).unwrap();

    assert_eq!(page.read(32, 6,).unwrap(), b"stable");
}

#[test]
fn truncated_tail_is_ignored() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.wal");

    {
        let mut log = LogManager::open(&path).unwrap();

        log.append_page_write(0, 0, b"complete").unwrap();

        log.flush().unwrap();
    }

    {
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();

        file.write_all(&[1, 2, 3, 4]).unwrap();

        file.sync_data().unwrap();
    }

    let mut log = LogManager::open(&path).unwrap();

    let records = log.records().unwrap();

    assert_eq!(records.len(), 1);

    assert_eq!(records[0].payload, b"complete");
}

#[test]
fn flush_through_makes_target_lsn_durable() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.wal");

    let mut log = LogManager::open(&path).unwrap();

    let first = log.append_page_write(0, 0, b"one").unwrap();

    let second = log.append_page_write(0, 8, b"two").unwrap();

    assert_eq!(log.durable_lsn(), None);

    log.flush_through(first).unwrap();

    assert!(
        log.durable_lsn()
            .is_some_and(|durable| { durable >= first },)
    );

    assert!(
        log.durable_lsn()
            .is_some_and(|durable| { durable >= second },)
    );
}

#[test]
fn flush_through_unknown_lsn_fails() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.wal");

    let mut log = LogManager::open(&path).unwrap();

    assert!(log.flush_through(0).is_err());
}
