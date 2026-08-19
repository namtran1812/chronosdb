use std::path::{Path, PathBuf};

use crate::recovery::RecoveryManager;
use crate::storage::{
    BufferPoolManager, BufferedHeapError, BufferedHeapFile, DiskManager, RecordId,
};
use crate::transaction::{
    DurableTransactionError, DurableTransactionManager, Transaction, TransactionId,
    TransactionState, TupleVersion,
};

const DEFAULT_BUFFER_POOL_SIZE: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum DurableTransactionalHeapError {
    #[error("heap operation failed: {0}")]
    Heap(#[from] BufferedHeapError),

    #[error("transaction operation failed: {0}")]
    Transaction(#[from] DurableTransactionError),

    #[error("recovery failed: {0}")]
    Recovery(#[from] crate::recovery::RecoveryError),

    #[error("disk I/O failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("transaction is not active")]
    NotActive,

    #[error("record is not visible to transaction")]
    RecordNotVisible,
}

pub struct DurableTransactionalHeap {
    heap: BufferedHeapFile,
    transactions: DurableTransactionManager,
    heap_path: PathBuf,
    wal_path: PathBuf,
}

impl DurableTransactionalHeap {
    pub fn open(
        heap_path: impl AsRef<Path>,
        wal_path: impl AsRef<Path>,
    ) -> Result<Self, DurableTransactionalHeapError> {
        Self::open_with_pool_size(heap_path, wal_path, DEFAULT_BUFFER_POOL_SIZE)
    }

    pub fn open_with_pool_size(
        heap_path: impl AsRef<Path>,
        wal_path: impl AsRef<Path>,
        pool_size: usize,
    ) -> Result<Self, DurableTransactionalHeapError> {
        let heap_path = heap_path.as_ref().to_path_buf();

        let wal_path = wal_path.as_ref().to_path_buf();

        let transactions = DurableTransactionManager::open(&wal_path)?;

        let shared_wal = transactions.shared_wal();

        let disk = DiskManager::open(&heap_path)?;

        /*
         * REDO all durable page WAL before the buffer
         * pool starts serving pages.
         */
        {
            let mut recovery_disk = DiskManager::open(&heap_path)?;

            let mut wal = shared_wal.borrow_mut();

            RecoveryManager::redo(&mut wal, &mut recovery_disk)?;
        }

        let buffer = BufferPoolManager::new_with_shared_wal(disk, shared_wal, pool_size);

        Ok(Self {
            heap: BufferedHeapFile::new(buffer),
            transactions,
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
        /*
         * Page-write WAL records were appended before the
         * commit record. Flushing COMMIT therefore makes
         * every preceding page mutation durable as well.
         */
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
        self.heap.flush_all()?;
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
