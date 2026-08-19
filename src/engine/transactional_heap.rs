use std::path::Path;

use crate::storage::{HeapFile, HeapFileError, RecordId};
use crate::transaction::{
    Transaction, TransactionError, TransactionId, TransactionManager, TransactionState,
    TupleVersion,
};

#[derive(Debug, thiserror::Error)]
pub enum TransactionalHeapError {
    #[error("heap operation failed: {0}")]
    Heap(#[from] HeapFileError),

    #[error("transaction operation failed: {0}")]
    Transaction(#[from] TransactionError),

    #[error("transaction is not active")]
    NotActive,

    #[error("record is not visible to transaction")]
    RecordNotVisible,

    #[error("write conflict with transaction {0}")]
    WriteConflict(TransactionId),
}

pub struct TransactionalHeap {
    heap: HeapFile,
    transactions: TransactionManager,
}

impl TransactionalHeap {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TransactionalHeapError> {
        Ok(Self {
            heap: HeapFile::open(path)?,
            transactions: TransactionManager::new(),
        })
    }

    pub fn begin(&mut self) -> Transaction {
        self.transactions.begin()
    }

    pub fn commit(&mut self, transaction_id: TransactionId) -> Result<(), TransactionalHeapError> {
        self.transactions.commit(transaction_id)?;
        Ok(())
    }

    pub fn abort(&mut self, transaction_id: TransactionId) -> Result<(), TransactionalHeapError> {
        self.transactions.abort(transaction_id)?;
        Ok(())
    }

    pub fn insert(
        &mut self,
        transaction_id: TransactionId,
        payload: Vec<u8>,
    ) -> Result<RecordId, TransactionalHeapError> {
        self.ensure_active(transaction_id)?;

        let version = TupleVersion::new(transaction_id, payload);

        Ok(self.heap.insert_version(&version)?)
    }

    pub fn get(&mut self, record_id: RecordId) -> Result<TupleVersion, TransactionalHeapError> {
        Ok(self.heap.get_version(record_id)?)
    }

    pub fn visible_scan(
        &mut self,
        transaction: &Transaction,
    ) -> Result<Vec<(RecordId, TupleVersion)>, TransactionalHeapError> {
        self.ensure_active(transaction.id())?;

        let transactions = &self.transactions;

        Ok(self
            .heap
            .visible_scan(transaction.snapshot(), transaction.id(), |transaction_id| {
                transactions.state(transaction_id)
            })?)
    }

    pub fn delete(
        &mut self,
        transaction: &Transaction,
        record_id: RecordId,
    ) -> Result<RecordId, TransactionalHeapError> {
        self.ensure_active(transaction.id())?;

        let mut version = self.heap.get_version(record_id)?;

        if let Some(owner) = version.conflicting_writer(transaction.id(), |transaction_id| {
            self.transactions.state(transaction_id)
        }) {
            return Err(TransactionalHeapError::WriteConflict(owner));
        }

        if !version.visible_to(transaction.snapshot(), transaction.id(), |transaction_id| {
            self.transactions.state(transaction_id)
        }) {
            return Err(TransactionalHeapError::RecordNotVisible);
        }

        version.mark_deleted(transaction.id());

        self.heap.replace_version(record_id, &version)?;

        Ok(record_id)
    }

    pub fn update(
        &mut self,
        transaction: &Transaction,
        record_id: RecordId,
        payload: Vec<u8>,
    ) -> Result<RecordId, TransactionalHeapError> {
        self.ensure_active(transaction.id())?;

        let mut old_version = self.heap.get_version(record_id)?;

        if let Some(owner) = old_version.conflicting_writer(transaction.id(), |transaction_id| {
            self.transactions.state(transaction_id)
        }) {
            return Err(TransactionalHeapError::WriteConflict(owner));
        }

        if !old_version.visible_to(transaction.snapshot(), transaction.id(), |transaction_id| {
            self.transactions.state(transaction_id)
        }) {
            return Err(TransactionalHeapError::RecordNotVisible);
        }

        old_version.mark_deleted(transaction.id());

        self.heap.replace_version(record_id, &old_version)?;

        let new_version = TupleVersion::new(transaction.id(), payload);

        Ok(self.heap.insert_version(&new_version)?)
    }

    pub fn sync(&mut self) -> Result<(), TransactionalHeapError> {
        self.heap.sync()?;
        Ok(())
    }

    pub fn page_count(&self) -> u64 {
        self.heap.page_count()
    }

    pub fn transaction_state(&self, transaction_id: TransactionId) -> Option<TransactionState> {
        self.transactions.state(transaction_id)
    }

    fn ensure_active(&self, transaction_id: TransactionId) -> Result<(), TransactionalHeapError> {
        match self.transactions.state(transaction_id) {
            Some(TransactionState::Active) => Ok(()),
            _ => Err(TransactionalHeapError::NotActive),
        }
    }
}
