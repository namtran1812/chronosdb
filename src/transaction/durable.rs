use std::path::Path;

use crate::recovery::{LogManager, recover_transactions};

use super::{Transaction, TransactionError, TransactionId, TransactionManager, TransactionState};

#[derive(Debug, thiserror::Error)]
pub enum DurableTransactionError {
    #[error("transaction error: {0}")]
    Transaction(#[from] TransactionError),

    #[error("WAL I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct DurableTransactionManager {
    manager: TransactionManager,
    wal: LogManager,
}

impl DurableTransactionManager {
    pub fn open(wal_path: impl AsRef<Path>) -> Result<Self, DurableTransactionError> {
        let mut wal = LogManager::open(wal_path)?;

        let recovered = recover_transactions(&mut wal)?;

        let manager = TransactionManager::from_recovered(
            recovered.states().clone(),
            recovered.next_transaction_id(),
        );

        Ok(Self { manager, wal })
    }

    pub fn begin(&mut self) -> Result<Transaction, DurableTransactionError> {
        let transaction = self.manager.begin();

        let lsn = self.wal.append_transaction_begin(transaction.id())?;

        self.wal.flush_through(lsn)?;

        Ok(transaction)
    }

    pub fn commit(&mut self, transaction_id: TransactionId) -> Result<(), DurableTransactionError> {
        self.manager.commit(transaction_id)?;

        let lsn = self.wal.append_transaction_commit(transaction_id)?;

        self.wal.flush_through(lsn)?;

        Ok(())
    }

    pub fn abort(&mut self, transaction_id: TransactionId) -> Result<(), DurableTransactionError> {
        self.manager.abort(transaction_id)?;

        let lsn = self.wal.append_transaction_abort(transaction_id)?;

        self.wal.flush_through(lsn)?;

        Ok(())
    }

    pub fn state(&self, transaction_id: TransactionId) -> Option<TransactionState> {
        self.manager.state(transaction_id)
    }

    pub fn manager(&self) -> &TransactionManager {
        &self.manager
    }
}
