use chronosdb::transaction::{TransactionError, TransactionManager, TransactionState};

#[test]
fn transaction_ids_are_monotonic() {
    let mut manager = TransactionManager::new();

    let first = manager.begin();

    let second = manager.begin();

    assert_eq!(first.id(), 1);

    assert_eq!(second.id(), 2);
}

#[test]
fn begin_marks_transaction_active() {
    let mut manager = TransactionManager::new();

    let tx = manager.begin();

    assert!(manager.is_active(tx.id(),));

    assert_eq!(manager.state(tx.id(),), Some(TransactionState::Active));
}

#[test]
fn commit_removes_transaction_from_active_set() {
    let mut manager = TransactionManager::new();

    let tx = manager.begin();

    manager.commit(tx.id()).unwrap();

    assert!(!manager.is_active(tx.id(),));

    assert_eq!(manager.state(tx.id(),), Some(TransactionState::Committed));
}

#[test]
fn abort_removes_transaction_from_active_set() {
    let mut manager = TransactionManager::new();

    let tx = manager.begin();

    manager.abort(tx.id()).unwrap();

    assert_eq!(manager.state(tx.id(),), Some(TransactionState::Aborted));
}

#[test]
fn completed_transaction_cannot_commit_twice() {
    let mut manager = TransactionManager::new();

    let tx = manager.begin();

    manager.commit(tx.id()).unwrap();

    assert_eq!(manager.commit(tx.id(),), Err(TransactionError::NotActive));
}

#[test]
fn unknown_transaction_fails() {
    let mut manager = TransactionManager::new();

    assert_eq!(
        manager.commit(999),
        Err(TransactionError::UnknownTransaction)
    );
}

#[test]
fn transaction_snapshot_captures_existing_active_transactions() {
    let mut manager = TransactionManager::new();

    let first = manager.begin();

    let second = manager.begin();

    assert!(second.snapshot().was_active(first.id(),));

    assert!(!second.snapshot().was_active(second.id(),));
}

#[test]
fn snapshot_is_stable_after_other_transaction_commits() {
    let mut manager = TransactionManager::new();

    let first = manager.begin();

    let second = manager.begin();

    manager.commit(first.id()).unwrap();

    assert!(second.snapshot().was_active(first.id(),));
}
