use std::collections::HashMap;

use crate::transaction::{TransactionId, TransactionState};

use super::{LogManager, LogRecordType};

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
    let records = log.records()?;

    let mut states = HashMap::new();

    let mut max_id = 0;

    for record in records {
        let Some(transaction_id) = record.transaction_id() else {
            continue;
        };

        max_id = max_id.max(transaction_id);

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
     * Any transaction that was active when the process crashed
     * is treated as aborted during restart.
     */
    for state in states.values_mut() {
        if *state == TransactionState::Active {
            *state = TransactionState::Aborted;
        }
    }

    Ok(RecoveredTransactions {
        states,
        next_transaction_id: max_id + 1,
    })
}
