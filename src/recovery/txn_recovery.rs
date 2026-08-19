use std::collections::HashMap;

use crate::transaction::{TransactionId, TransactionState};

use super::{Checkpoint, LogManager, LogRecord, LogRecordType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredTransactions {
    states: HashMap<TransactionId, TransactionState>,
    next_transaction_id: TransactionId,
}

impl RecoveredTransactions {
    pub fn state(&self, transaction_id: TransactionId) -> Option<TransactionState> {
        self.states.get(&transaction_id).copied()
    }

    pub fn states(&self) -> &HashMap<TransactionId, TransactionState> {
        &self.states
    }

    pub fn next_transaction_id(&self) -> TransactionId {
        self.next_transaction_id
    }
}

pub fn recover_transactions(log: &mut LogManager) -> std::io::Result<RecoveredTransactions> {
    recover_from_records(HashMap::new(), 1, log.records()?)
}

pub fn recover_transactions_after_checkpoint(
    checkpoint: &Checkpoint,
    log: &mut LogManager,
) -> std::io::Result<RecoveredTransactions> {
    recover_from_records(
        checkpoint.states().clone(),
        checkpoint.next_transaction_id(),
        log.records_after(checkpoint.lsn())?,
    )
}

fn recover_from_records(
    mut states: HashMap<TransactionId, TransactionState>,
    mut next_transaction_id: TransactionId,
    records: Vec<LogRecord>,
) -> std::io::Result<RecoveredTransactions> {
    for record in records {
        let Some(transaction_id) = record.transaction_id() else {
            continue;
        };

        next_transaction_id = next_transaction_id.max(transaction_id + 1);

        let state = match record.record_type {
            LogRecordType::TransactionBegin => TransactionState::Active,

            LogRecordType::TransactionCommit => TransactionState::Committed,

            LogRecordType::TransactionAbort => TransactionState::Aborted,

            LogRecordType::PageWrite => {
                continue;
            }
        };

        states.insert(transaction_id, state);
    }

    /*
     * Any transaction still active at recovery time
     * is considered crash-aborted.
     */
    for state in states.values_mut() {
        if *state == TransactionState::Active {
            *state = TransactionState::Aborted;
        }
    }

    Ok(RecoveredTransactions {
        states,
        next_transaction_id,
    })
}
