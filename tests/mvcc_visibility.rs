use chronosdb::transaction::{TransactionManager, TransactionState, transaction_visible};

#[test]
fn transaction_sees_its_own_write() {
    let mut manager = TransactionManager::new();

    let tx = manager.begin();

    assert!(transaction_visible(
        tx.id(),
        TransactionState::Active,
        tx.snapshot(),
        tx.id(),
    ));
}

#[test]
fn aborted_self_write_is_not_visible() {
    let mut manager = TransactionManager::new();

    let tx = manager.begin();

    assert!(!transaction_visible(
        tx.id(),
        TransactionState::Aborted,
        tx.snapshot(),
        tx.id(),
    ));
}

#[test]
fn uncommitted_other_transaction_is_not_visible() {
    let mut manager = TransactionManager::new();

    let writer = manager.begin();

    let reader = manager.begin();

    assert!(!transaction_visible(
        writer.id(),
        TransactionState::Active,
        reader.snapshot(),
        reader.id(),
    ));
}

#[test]
fn transaction_active_at_snapshot_time_stays_invisible() {
    let mut manager = TransactionManager::new();

    let writer = manager.begin();

    let reader = manager.begin();

    manager.commit(writer.id()).unwrap();

    assert!(!transaction_visible(
        writer.id(),
        TransactionState::Committed,
        reader.snapshot(),
        reader.id(),
    ));
}

#[test]
fn committed_transaction_before_snapshot_is_visible() {
    let mut manager = TransactionManager::new();

    let writer = manager.begin();

    manager.commit(writer.id()).unwrap();

    let reader = manager.begin();

    assert!(transaction_visible(
        writer.id(),
        TransactionState::Committed,
        reader.snapshot(),
        reader.id(),
    ));
}

#[test]
fn transaction_started_after_snapshot_is_invisible() {
    let mut manager = TransactionManager::new();

    let reader = manager.begin();

    let future_writer = manager.begin();

    manager.commit(future_writer.id()).unwrap();

    assert!(!transaction_visible(
        future_writer.id(),
        TransactionState::Committed,
        reader.snapshot(),
        reader.id(),
    ));
}

#[test]
fn new_reader_sees_commit_old_reader_does_not() {
    let mut manager = TransactionManager::new();

    let writer = manager.begin();

    let old_reader = manager.begin();

    manager.commit(writer.id()).unwrap();

    let new_reader = manager.begin();

    assert!(!transaction_visible(
        writer.id(),
        TransactionState::Committed,
        old_reader.snapshot(),
        old_reader.id(),
    ));

    assert!(transaction_visible(
        writer.id(),
        TransactionState::Committed,
        new_reader.snapshot(),
        new_reader.id(),
    ));
}
