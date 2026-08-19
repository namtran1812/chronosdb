use chronosdb::storage::{BufferPoolError, BufferPoolManager, DiskManager};

fn manager(pool_size: usize) -> (tempfile::TempDir, BufferPoolManager) {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.db");

    let disk = DiskManager::open(path).unwrap();

    (directory, BufferPoolManager::new(disk, pool_size))
}

#[test]
fn new_page_is_resident_and_pinned() {
    let (_directory, mut buffer) = manager(2);

    let page_id = buffer.new_page().unwrap();

    assert!(buffer.is_resident(page_id));

    assert_eq!(buffer.pin_count(page_id), Some(1));
}

#[test]
fn unpin_decrements_pin_count() {
    let (_directory, mut buffer) = manager(2);

    let page_id = buffer.new_page().unwrap();

    buffer.unpin_page(page_id, false).unwrap();

    assert_eq!(buffer.pin_count(page_id), Some(0));
}

#[test]
fn unpin_underflow_is_rejected() {
    let (_directory, mut buffer) = manager(2);

    let page_id = buffer.new_page().unwrap();

    buffer.unpin_page(page_id, false).unwrap();

    assert_eq!(
        buffer.unpin_page(page_id, false,),
        Err(BufferPoolError::PinCountUnderflow)
    );
}

#[test]
fn all_pinned_frames_prevent_eviction() {
    let (_directory, mut buffer) = manager(2);

    buffer.new_page().unwrap();

    buffer.new_page().unwrap();

    assert_eq!(buffer.new_page(), Err(BufferPoolError::NoFrameAvailable));
}

#[test]
fn unpinned_page_can_be_evicted() {
    let (_directory, mut buffer) = manager(1);

    let first = buffer.new_page().unwrap();

    buffer.unpin_page(first, false).unwrap();

    let second = buffer.new_page().unwrap();

    assert_ne!(first, second);

    assert!(!buffer.is_resident(first));

    assert!(buffer.is_resident(second));
}

#[test]
fn dirty_page_survives_eviction() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.db");

    {
        let disk = DiskManager::open(&path).unwrap();

        let mut buffer = BufferPoolManager::new(disk, 1);

        let first = buffer.new_page().unwrap();

        {
            let page = buffer.fetch_page_mut(first).unwrap();

            page.write(32, b"persistent").unwrap();
        }

        buffer.unpin_page(first, true).unwrap();

        buffer.unpin_page(first, true).unwrap();

        let second = buffer.new_page().unwrap();

        assert_ne!(first, second);
    }

    {
        let mut disk = DiskManager::open(&path).unwrap();

        let page = disk.read_page(0).unwrap();

        assert_eq!(page.read(32, 10,).unwrap(), b"persistent");
    }
}

#[test]
fn cached_fetch_increases_pin_count() {
    let (_directory, mut buffer) = manager(2);

    let page_id = buffer.new_page().unwrap();

    buffer.unpin_page(page_id, false).unwrap();

    buffer.fetch_page(page_id).unwrap();

    assert_eq!(buffer.pin_count(page_id), Some(1));
}

#[test]
fn least_recently_used_unpinned_page_is_evicted() {
    let (_directory, mut buffer) = manager(2);

    let first = buffer.new_page().unwrap();

    let second = buffer.new_page().unwrap();

    buffer.unpin_page(first, false).unwrap();

    buffer.unpin_page(second, false).unwrap();

    buffer.fetch_page(second).unwrap();

    buffer.unpin_page(second, false).unwrap();

    let third = buffer.new_page().unwrap();

    assert!(!buffer.is_resident(first));

    assert!(buffer.is_resident(second));

    assert!(buffer.is_resident(third));
}

#[test]
fn flush_all_persists_dirty_pages() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.db");

    {
        let disk = DiskManager::open(&path).unwrap();

        let mut buffer = BufferPoolManager::new(disk, 2);

        let page_id = buffer.new_page().unwrap();

        {
            let page = buffer.fetch_page_mut(page_id).unwrap();

            page.write(100, b"chronos").unwrap();
        }

        buffer.unpin_page(page_id, true).unwrap();

        buffer.unpin_page(page_id, true).unwrap();

        buffer.flush_all().unwrap();
    }

    let mut disk = DiskManager::open(&path).unwrap();

    let page = disk.read_page(0).unwrap();

    assert_eq!(page.read(100, 7,).unwrap(), b"chronos");
}
