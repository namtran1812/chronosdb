use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use crate::recovery::{
    Checkpoint, LogManager, recover_transactions, recover_transactions_after_checkpoint,
};

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
    wal: Rc<RefCell<LogManager>>,
}

impl DurableTransactionManager {
    pub fn open(wal_path: impl AsRef<Path>) -> Result<Self, DurableTransactionError> {
        Self::open_with_checkpoint(wal_path, None)
    }

    pub fn open_with_checkpoint(
        wal_path: impl AsRef<Path>,
        checkpoint: Option<&Checkpoint>,
    ) -> Result<Self, DurableTransactionError> {
        let mut wal = LogManager::open(wal_path)?;

        let recovered = match checkpoint {
            Some(checkpoint) => recover_transactions_after_checkpoint(checkpoint, &mut wal)?,

            None => recover_transactions(&mut wal)?,
        };

        let manager = TransactionManager::from_recovered(
            recovered.states().clone(),
            recovered.next_transaction_id(),
        );

        Ok(Self {
            manager,
            wal: Rc::new(RefCell::new(wal)),
        })
    }

    pub fn begin(&mut self) -> Result<Transaction, DurableTransactionError> {
        let transaction = self.manager.begin();

        let lsn = self
            .wal
            .borrow_mut()
            .append_transaction_begin(transaction.id())?;

        self.wal.borrow_mut().flush_through(lsn)?;

        Ok(transaction)
    }

    pub fn commit(&mut self, transaction_id: TransactionId) -> Result<(), DurableTransactionError> {
        if self.manager.state(transaction_id) != Some(TransactionState::Active) {
            self.manager.commit(transaction_id)?;

            unreachable!("commit validation should have returned");
        }

        let lsn = self
            .wal
            .borrow_mut()
            .append_transaction_commit(transaction_id)?;

        self.wal.borrow_mut().flush_through(lsn)?;

        self.manager.commit(transaction_id)?;

        Ok(())
    }

    pub fn abort(&mut self, transaction_id: TransactionId) -> Result<(), DurableTransactionError> {
        if self.manager.state(transaction_id) != Some(TransactionState::Active) {
            self.manager.abort(transaction_id)?;

            unreachable!("abort validation should have returned");
        }

        let lsn = self
            .wal
            .borrow_mut()
            .append_transaction_abort(transaction_id)?;

        self.wal.borrow_mut().flush_through(lsn)?;

        self.manager.abort(transaction_id)?;

        Ok(())
    }

    pub fn state(&self, transaction_id: TransactionId) -> Option<TransactionState> {
        self.manager.state(transaction_id)
    }

    pub fn manager(&self) -> &TransactionManager {
        &self.manager
    }

    pub fn shared_wal(&self) -> Rc<RefCell<LogManager>> {
        Rc::clone(&self.wal)
    }

    pub fn latest_lsn(&self) -> Option<crate::recovery::Lsn> {
        self.wal.borrow().next_lsn().checked_sub(1)
    }

    pub fn oldest_active_xmin(&self) -> TransactionId {
        self.manager.oldest_active_xmin()
    }

    pub fn compact_wal_through(
        &mut self,
        checkpoint_lsn: crate::recovery::Lsn,
    ) -> Result<usize, DurableTransactionError> {
        Ok(self.wal.borrow_mut().compact_through(checkpoint_lsn)?)
    }
}
