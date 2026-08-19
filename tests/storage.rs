use chronosdb::PAGE_SIZE;
use chronosdb::storage::{DiskManager, Page};

#[test]
fn page_starts_clean() {
    let page = Page::new(7);

    assert_eq!(page.id(), 7);
    assert!(!page.is_dirty());
}

#[test]
fn page_write_marks_dirty() {
    let mut page = Page::new(0);

    page.write(32, b"chronos").unwrap();

    assert!(page.is_dirty());

    assert_eq!(page.read(32, 7).unwrap(), b"chronos",);
}

#[test]
fn page_rejects_out_of_bounds_write() {
    let mut page = Page::new(0);

    assert!(page.write(PAGE_SIZE - 1, &[1, 2],).is_err());
}

#[test]
fn page_rejects_out_of_bounds_read() {
    let page = Page::new(0);

    assert!(page.read(PAGE_SIZE - 1, 2,).is_err());
}

#[test]
fn allocated_pages_have_monotonic_ids() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.db");

    let mut disk = DiskManager::open(path).unwrap();

    let first = disk.allocate_page().unwrap();

    let second = disk.allocate_page().unwrap();

    assert_eq!(first.id(), 0);
    assert_eq!(second.id(), 1);
    assert_eq!(disk.page_count(), 2);
}

#[test]
fn page_survives_disk_round_trip() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.db");

    {
        let mut disk = DiskManager::open(&path).unwrap();

        let mut page = disk.allocate_page().unwrap();

        page.write(128, b"chronosdb").unwrap();

        disk.write_page(&page).unwrap();

        disk.sync().unwrap();
    }

    {
        let mut disk = DiskManager::open(&path).unwrap();

        assert_eq!(disk.page_count(), 1);

        let page = disk.read_page(0).unwrap();

        assert_eq!(page.read(128, 9).unwrap(), b"chronosdb",);

        assert!(!page.is_dirty());
    }
}

#[test]
fn reopening_database_preserves_page_count() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.db");

    {
        let mut disk = DiskManager::open(&path).unwrap();

        for _ in 0..5 {
            disk.allocate_page().unwrap();
        }

        disk.sync().unwrap();
    }

    let disk = DiskManager::open(&path).unwrap();

    assert_eq!(disk.page_count(), 5);
}
