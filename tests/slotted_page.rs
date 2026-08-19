use chronosdb::PAGE_SIZE;
use chronosdb::storage::{SlottedPage, SlottedPageError};

#[test]
fn new_page_is_empty() {
    let page = SlottedPage::new();

    assert_eq!(page.slot_count(), 0);

    assert_eq!(page.free_space(), PAGE_SIZE - 6);
}

#[test]
fn insert_and_read_record() {
    let mut page = SlottedPage::new();

    let slot = page.insert(b"hello").unwrap();

    assert_eq!(slot, 0);

    assert_eq!(page.get(slot).unwrap(), b"hello");
}

#[test]
fn multiple_records_have_stable_slots() {
    let mut page = SlottedPage::new();

    let first = page.insert(b"alpha").unwrap();

    let second = page.insert(b"beta").unwrap();

    assert_eq!(first, 0);
    assert_eq!(second, 1);

    assert_eq!(page.get(first).unwrap(), b"alpha");

    assert_eq!(page.get(second).unwrap(), b"beta");
}

#[test]
fn delete_marks_slot_deleted() {
    let mut page = SlottedPage::new();

    let slot = page.insert(b"hello").unwrap();

    page.delete(slot).unwrap();

    assert_eq!(page.get(slot), Err(SlottedPageError::Deleted));
}

#[test]
fn deleted_slot_is_reused() {
    let mut page = SlottedPage::new();

    let first = page.insert(b"alpha").unwrap();

    let second = page.insert(b"beta").unwrap();

    page.delete(first).unwrap();

    let replacement = page.insert(b"gamma").unwrap();

    assert_eq!(replacement, first);

    assert_eq!(page.get(replacement).unwrap(), b"gamma");

    assert_eq!(page.get(second).unwrap(), b"beta");
}

#[test]
fn invalid_slot_fails() {
    let page = SlottedPage::new();

    assert_eq!(page.get(99), Err(SlottedPageError::InvalidSlot));
}

#[test]
fn compaction_preserves_live_records() {
    let mut page = SlottedPage::new();

    let first = page.insert(&vec![1; 500]).unwrap();

    let second = page.insert(&vec![2; 500]).unwrap();

    let third = page.insert(&vec![3; 500]).unwrap();

    page.delete(second).unwrap();

    let before = page.free_space();

    page.compact();

    let after = page.free_space();

    assert!(after > before);

    assert_eq!(page.get(first).unwrap(), vec![1; 500]);

    assert_eq!(page.get(third).unwrap(), vec![3; 500]);
}

#[test]
fn page_round_trip_preserves_slots() {
    let mut page = SlottedPage::new();

    let first = page.insert(b"chronos").unwrap();

    let second = page.insert(b"database").unwrap();

    let bytes = Box::new(*page.as_bytes());

    let restored = SlottedPage::from_bytes(bytes);

    assert_eq!(restored.get(first).unwrap(), b"chronos");

    assert_eq!(restored.get(second).unwrap(), b"database");
}

#[test]
fn fills_page_until_no_space() {
    let mut page = SlottedPage::new();

    let record = vec![7_u8; 128];

    let mut inserted = 0;

    loop {
        match page.insert(&record) {
            Ok(_) => {
                inserted += 1;
            }
            Err(SlottedPageError::NoSpace) => {
                break;
            }
            Err(error) => {
                panic!("unexpected error: {error}");
            }
        }
    }

    assert!(inserted > 20);
}
