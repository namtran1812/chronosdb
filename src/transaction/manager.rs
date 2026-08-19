use std::collections::{BTreeSet, HashMap};

use super::{Snapshot, TransactionState};

pub type TransactionId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    id: TransactionId,
    state: TransactionState,
    snapshot: Snapshot,
}

impl Transaction {
    pub fn id(&self) -> TransactionId {
        self.id
    }

    pub fn state(&self) -> TransactionState {
        self.state
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TransactionError {
    #[error("transaction does not exist")]
    UnknownTransaction,

    #[error("transaction is not active")]
    NotActive,
}

pub struct TransactionManager {
    next_transaction_id: TransactionId,

    states: HashMap<TransactionId, TransactionState>,

    active: BTreeSet<TransactionId>,
}

impl Default for TransactionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TransactionManager {
    pub fn new() -> Self {
        Self {
            next_transaction_id: 1,
            states: HashMap::new(),
            active: BTreeSet::new(),
        }
    }

    pub fn from_recovered(
        states: HashMap<TransactionId, TransactionState>,
        next_transaction_id: TransactionId,
    ) -> Self {
        let active = states
            .iter()
            .filter_map(|(transaction_id, state)| {
                (*state == TransactionState::Active).then_some(*transaction_id)
            })
            .collect();

        Self {
            next_transaction_id,
            states,
            active,
        }
    }

    pub fn begin(&mut self) -> Transaction {
        let id = self.next_transaction_id;

        self.next_transaction_id += 1;

        let snapshot = self.current_snapshot();

        self.states.insert(id, TransactionState::Active);

        self.active.insert(id);

        Transaction {
            id,
            state: TransactionState::Active,
            snapshot,
        }
    }

    pub fn commit(&mut self, transaction_id: TransactionId) -> Result<(), TransactionError> {
        self.finish(transaction_id, TransactionState::Committed)
    }

    pub fn abort(&mut self, transaction_id: TransactionId) -> Result<(), TransactionError> {
        self.finish(transaction_id, TransactionState::Aborted)
    }

    pub fn state(&self, transaction_id: TransactionId) -> Option<TransactionState> {
        self.states.get(&transaction_id).copied()
    }

    pub fn states(&self) -> &HashMap<TransactionId, TransactionState> {
        &self.states
    }

    pub fn next_transaction_id(&self) -> TransactionId {
        self.next_transaction_id
    }

    pub fn is_active(&self, transaction_id: TransactionId) -> bool {
        self.active.contains(&transaction_id)
    }

    pub fn active_transactions(&self) -> &BTreeSet<TransactionId> {
        &self.active
    }

    pub fn oldest_active_xmin(&self) -> TransactionId {
        self.active
            .iter()
            .next()
            .copied()
            .unwrap_or(self.next_transaction_id)
    }

    pub fn current_snapshot(&self) -> Snapshot {
        let xmin = self
            .active
            .iter()
            .next()
            .copied()
            .unwrap_or(self.next_transaction_id);

        Snapshot::new(xmin, self.next_transaction_id, self.active.clone())
    }

    fn finish(
        &mut self,
        transaction_id: TransactionId,
        final_state: TransactionState,
    ) -> Result<(), TransactionError> {
        let state = self
            .states
            .get_mut(&transaction_id)
            .ok_or(TransactionError::UnknownTransaction)?;

        if *state != TransactionState::Active {
            return Err(TransactionError::NotActive);
        }

        *state = final_state;

        self.active.remove(&transaction_id);

        Ok(())
    }
}
