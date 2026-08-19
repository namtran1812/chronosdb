use chronosdb::transaction::{DurableTransactionManager, TransactionState};

#[test]
fn committed_transaction_survives_restart() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.wal");

    let transaction_id;

    {
        let mut manager = DurableTransactionManager::open(&path).unwrap();

        let transaction = manager.begin().unwrap();

        transaction_id = transaction.id();

        manager.commit(transaction_id).unwrap();
    }

    let manager = DurableTransactionManager::open(&path).unwrap();

    assert_eq!(
        manager.state(transaction_id,),
        Some(TransactionState::Committed)
    );
}

#[test]
fn aborted_transaction_survives_restart() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.wal");

    let transaction_id;

    {
        let mut manager = DurableTransactionManager::open(&path).unwrap();

        let transaction = manager.begin().unwrap();

        transaction_id = transaction.id();

        manager.abort(transaction_id).unwrap();
    }

    let manager = DurableTransactionManager::open(&path).unwrap();

    assert_eq!(
        manager.state(transaction_id,),
        Some(TransactionState::Aborted)
    );
}

#[test]
fn active_transaction_is_aborted_after_crash() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.wal");

    let transaction_id;

    {
        let mut manager = DurableTransactionManager::open(&path).unwrap();

        transaction_id = manager.begin().unwrap().id();

        /*
         * Simulated crash:
         * no commit or abort record.
         */
    }

    let manager = DurableTransactionManager::open(&path).unwrap();

    assert_eq!(
        manager.state(transaction_id,),
        Some(TransactionState::Aborted)
    );
}

#[test]
fn transaction_ids_continue_after_restart() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.wal");

    {
        let mut manager = DurableTransactionManager::open(&path).unwrap();

        let first = manager.begin().unwrap();

        manager.commit(first.id()).unwrap();
    }

    let mut manager = DurableTransactionManager::open(&path).unwrap();

    let second = manager.begin().unwrap();

    assert_eq!(second.id(), 2);
}

#[test]
fn multiple_transaction_states_recover() {
    let directory = tempfile::tempdir().unwrap();

    let path = directory.path().join("chronos.wal");

    {
        let mut manager = DurableTransactionManager::open(&path).unwrap();

        let committed = manager.begin().unwrap();

        manager.commit(committed.id()).unwrap();

        let aborted = manager.begin().unwrap();

        manager.abort(aborted.id()).unwrap();

        let _crashed = manager.begin().unwrap();
    }

    let manager = DurableTransactionManager::open(&path).unwrap();

    assert_eq!(manager.state(1), Some(TransactionState::Committed));

    assert_eq!(manager.state(2), Some(TransactionState::Aborted));

    assert_eq!(manager.state(3), Some(TransactionState::Aborted));
}
