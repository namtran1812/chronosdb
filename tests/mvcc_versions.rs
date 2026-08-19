use chronosdb::transaction::{TransactionManager, TransactionState, TupleVersion, VersionChain};

#[test]
fn uncommitted_insert_is_hidden_from_other_transaction() {
    let mut manager = TransactionManager::new();

    let writer = manager.begin();

    let reader = manager.begin();

    let version = TupleVersion::new(writer.id(), b"draft".to_vec());

    assert!(!version.visible_to(reader.snapshot(), reader.id(), |txid| {
        manager.state(txid)
    },));
}

#[test]
fn writer_sees_own_uncommitted_insert() {
    let mut manager = TransactionManager::new();

    let writer = manager.begin();

    let version = TupleVersion::new(writer.id(), b"draft".to_vec());

    assert!(version.visible_to(writer.snapshot(), writer.id(), |txid| {
        manager.state(txid)
    },));
}

#[test]
fn committed_insert_is_visible_to_new_reader() {
    let mut manager = TransactionManager::new();

    let writer = manager.begin();

    let version = TupleVersion::new(writer.id(), b"committed".to_vec());

    manager.commit(writer.id()).unwrap();

    let reader = manager.begin();

    assert!(version.visible_to(reader.snapshot(), reader.id(), |txid| {
        manager.state(txid)
    },));
}

#[test]
fn aborted_insert_is_invisible() {
    let mut manager = TransactionManager::new();

    let writer = manager.begin();

    let version = TupleVersion::new(writer.id(), b"aborted".to_vec());

    manager.abort(writer.id()).unwrap();

    let reader = manager.begin();

    assert!(!version.visible_to(reader.snapshot(), reader.id(), |txid| {
        manager.state(txid)
    },));
}

#[test]
fn old_reader_sees_old_version_after_update_commits() {
    let mut manager = TransactionManager::new();

    let creator = manager.begin();

    let mut chain = VersionChain::new();

    chain.insert(creator.id(), b"old".to_vec());

    manager.commit(creator.id()).unwrap();

    let old_reader = manager.begin();

    let updater = manager.begin();

    chain
        .update(updater.snapshot(), updater.id(), b"new".to_vec(), |txid| {
            manager.state(txid)
        })
        .unwrap();

    manager.commit(updater.id()).unwrap();

    let visible = chain
        .visible_version(old_reader.snapshot(), old_reader.id(), |txid| {
            manager.state(txid)
        })
        .unwrap();

    assert_eq!(visible.payload(), b"old");
}

#[test]
fn new_reader_sees_new_version_after_update_commit() {
    let mut manager = TransactionManager::new();

    let creator = manager.begin();

    let mut chain = VersionChain::new();

    chain.insert(creator.id(), b"old".to_vec());

    manager.commit(creator.id()).unwrap();

    let updater = manager.begin();

    chain
        .update(updater.snapshot(), updater.id(), b"new".to_vec(), |txid| {
            manager.state(txid)
        })
        .unwrap();

    manager.commit(updater.id()).unwrap();

    let reader = manager.begin();

    let visible = chain
        .visible_version(reader.snapshot(), reader.id(), |txid| manager.state(txid))
        .unwrap();

    assert_eq!(visible.payload(), b"new");
}

#[test]
fn writer_sees_own_update_before_commit() {
    let mut manager = TransactionManager::new();

    let creator = manager.begin();

    let mut chain = VersionChain::new();

    chain.insert(creator.id(), b"old".to_vec());

    manager.commit(creator.id()).unwrap();

    let updater = manager.begin();

    chain
        .update(updater.snapshot(), updater.id(), b"new".to_vec(), |txid| {
            manager.state(txid)
        })
        .unwrap();

    let visible = chain
        .visible_version(updater.snapshot(), updater.id(), |txid| manager.state(txid))
        .unwrap();

    assert_eq!(visible.payload(), b"new");
}

#[test]
fn delete_does_not_affect_older_snapshot() {
    let mut manager = TransactionManager::new();

    let creator = manager.begin();

    let mut chain = VersionChain::new();

    chain.insert(creator.id(), b"alive".to_vec());

    manager.commit(creator.id()).unwrap();

    let old_reader = manager.begin();

    let deleter = manager.begin();

    chain
        .delete(deleter.snapshot(), deleter.id(), |txid| manager.state(txid))
        .unwrap();

    manager.commit(deleter.id()).unwrap();

    assert!(
        chain
            .visible_version(old_reader.snapshot(), old_reader.id(), |txid| {
                manager.state(txid)
            },)
            .is_some()
    );
}

#[test]
fn delete_hides_tuple_from_new_snapshot() {
    let mut manager = TransactionManager::new();

    let creator = manager.begin();

    let mut chain = VersionChain::new();

    chain.insert(creator.id(), b"alive".to_vec());

    manager.commit(creator.id()).unwrap();

    let deleter = manager.begin();

    chain
        .delete(deleter.snapshot(), deleter.id(), |txid| manager.state(txid))
        .unwrap();

    manager.commit(deleter.id()).unwrap();

    let reader = manager.begin();

    assert!(
        chain
            .visible_version(reader.snapshot(), reader.id(), |txid| {
                manager.state(txid)
            },)
            .is_none()
    );
}

#[test]
fn aborted_delete_does_not_hide_tuple() {
    let mut manager = TransactionManager::new();

    let creator = manager.begin();

    let mut chain = VersionChain::new();

    chain.insert(creator.id(), b"alive".to_vec());

    manager.commit(creator.id()).unwrap();

    let deleter = manager.begin();

    chain
        .delete(deleter.snapshot(), deleter.id(), |txid| manager.state(txid))
        .unwrap();

    manager.abort(deleter.id()).unwrap();

    let reader = manager.begin();

    let visible = chain
        .visible_version(reader.snapshot(), reader.id(), |txid| manager.state(txid))
        .unwrap();

    assert_eq!(visible.payload(), b"alive");
}

#[test]
fn aborted_update_does_not_replace_committed_version() {
    let mut manager = TransactionManager::new();

    let creator = manager.begin();

    let mut chain = VersionChain::new();

    chain.insert(creator.id(), b"old".to_vec());

    manager.commit(creator.id()).unwrap();

    let updater = manager.begin();

    chain
        .update(updater.snapshot(), updater.id(), b"bad".to_vec(), |txid| {
            manager.state(txid)
        })
        .unwrap();

    manager.abort(updater.id()).unwrap();

    let reader = manager.begin();

    let visible = chain
        .visible_version(reader.snapshot(), reader.id(), |txid| manager.state(txid))
        .unwrap();

    assert_eq!(visible.payload(), b"old");
}

#[test]
fn transaction_state_lookup_is_used_for_visibility() {
    let mut manager = TransactionManager::new();

    let writer = manager.begin();

    let version = TupleVersion::new(writer.id(), b"value".to_vec());

    manager.commit(writer.id()).unwrap();

    let reader = manager.begin();

    assert!(version.visible_to(reader.snapshot(), reader.id(), |txid| {
        if txid == writer.id() {
            Some(TransactionState::Committed)
        } else {
            manager.state(txid)
        }
    },));
}
