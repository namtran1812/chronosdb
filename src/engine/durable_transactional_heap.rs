use std::path::{Path, PathBuf};

use crate::storage::{HeapFile, HeapFileError, RecordId};
use crate::transaction::{
    DurableTransactionError, DurableTransactionManager, Transaction, TransactionId,
    TransactionState, TupleVersion,
};

#[derive(Debug, thiserror::Error)]
pub enum DurableTransactionalHeapError {
    #[error("heap operation failed: {0}")]
    Heap(#[from] HeapFileError),

    #[error("transaction operation failed: {0}")]
    Transaction(#[from] DurableTransactionError),

    #[error("transaction is not active")]
    NotActive,

    #[error("record is not visible to transaction")]
    RecordNotVisible,
}

pub struct DurableTransactionalHeap {
    heap: HeapFile,
    transactions: DurableTransactionManager,
    heap_path: PathBuf,
    wal_path: PathBuf,
}

impl DurableTransactionalHeap {
    pub fn open(
        heap_path: impl AsRef<Path>,
        wal_path: impl AsRef<Path>,
    ) -> Result<Self, DurableTransactionalHeapError> {
        let heap_path = heap_path.as_ref().to_path_buf();

        let wal_path = wal_path.as_ref().to_path_buf();

        Ok(Self {
            heap: HeapFile::open(&heap_path)?,
            transactions: DurableTransactionManager::open(&wal_path)?,
            heap_path,
            wal_path,
        })
    }

    pub fn begin(&mut self) -> Result<Transaction, DurableTransactionalHeapError> {
        Ok(self.transactions.begin()?)
    }

    pub fn commit(
        &mut self,
        transaction_id: TransactionId,
    ) -> Result<(), DurableTransactionalHeapError> {
        self.transactions.commit(transaction_id)?;

        Ok(())
    }

    pub fn abort(
        &mut self,
        transaction_id: TransactionId,
    ) -> Result<(), DurableTransactionalHeapError> {
        self.transactions.abort(transaction_id)?;

        Ok(())
    }

    pub fn insert(
        &mut self,
        transaction_id: TransactionId,
        payload: Vec<u8>,
    ) -> Result<RecordId, DurableTransactionalHeapError> {
        self.ensure_active(transaction_id)?;

        let version = TupleVersion::new(transaction_id, payload);

        Ok(self.heap.insert_version(&version)?)
    }

    pub fn update(
        &mut self,
        transaction: &Transaction,
        record_id: RecordId,
        payload: Vec<u8>,
    ) -> Result<RecordId, DurableTransactionalHeapError> {
        self.ensure_active(transaction.id())?;

        let mut old_version = self.heap.get_version(record_id)?;

        if !old_version.visible_to(transaction.snapshot(), transaction.id(), |transaction_id| {
            self.transactions.state(transaction_id)
        }) {
            return Err(DurableTransactionalHeapError::RecordNotVisible);
        }

        old_version.mark_deleted(transaction.id());

        self.heap.replace_version(record_id, &old_version)?;

        let new_version = TupleVersion::new(transaction.id(), payload);

        Ok(self.heap.insert_version(&new_version)?)
    }

    pub fn delete(
        &mut self,
        transaction: &Transaction,
        record_id: RecordId,
    ) -> Result<RecordId, DurableTransactionalHeapError> {
        self.ensure_active(transaction.id())?;

        let mut version = self.heap.get_version(record_id)?;

        if !version.visible_to(transaction.snapshot(), transaction.id(), |transaction_id| {
            self.transactions.state(transaction_id)
        }) {
            return Err(DurableTransactionalHeapError::RecordNotVisible);
        }

        version.mark_deleted(transaction.id());

        self.heap.replace_version(record_id, &version)?;

        Ok(record_id)
    }

    pub fn visible_scan(
        &mut self,
        transaction: &Transaction,
    ) -> Result<Vec<(RecordId, TupleVersion)>, DurableTransactionalHeapError> {
        self.ensure_active(transaction.id())?;

        let transactions = &self.transactions;

        Ok(self
            .heap
            .visible_scan(transaction.snapshot(), transaction.id(), |transaction_id| {
                transactions.state(transaction_id)
            })?)
    }

    pub fn get(
        &mut self,
        record_id: RecordId,
    ) -> Result<TupleVersion, DurableTransactionalHeapError> {
        Ok(self.heap.get_version(record_id)?)
    }

    pub fn sync(&mut self) -> Result<(), DurableTransactionalHeapError> {
        self.heap.sync()?;

        Ok(())
    }

    pub fn page_count(&self) -> u64 {
        self.heap.page_count()
    }

    pub fn transaction_state(&self, transaction_id: TransactionId) -> Option<TransactionState> {
        self.transactions.state(transaction_id)
    }

    pub fn heap_path(&self) -> &Path {
        &self.heap_path
    }

    pub fn wal_path(&self) -> &Path {
        &self.wal_path
    }

    fn ensure_active(
        &self,
        transaction_id: TransactionId,
    ) -> Result<(), DurableTransactionalHeapError> {
        match self.transactions.state(transaction_id) {
            Some(TransactionState::Active) => Ok(()),

            _ => Err(DurableTransactionalHeapError::NotActive),
        }
    }
}
