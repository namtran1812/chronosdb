use super::{Snapshot, TransactionId, TransactionState, transaction_visible};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TupleVersion {
    xmin: TransactionId,
    xmax: Option<TransactionId>,
    payload: Vec<u8>,
}

impl TupleVersion {
    pub fn new(xmin: TransactionId, payload: Vec<u8>) -> Self {
        Self {
            xmin,
            xmax: None,
            payload,
        }
    }

    pub fn xmin(&self) -> TransactionId {
        self.xmin
    }

    pub fn xmax(&self) -> Option<TransactionId> {
        self.xmax
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn mark_deleted(&mut self, transaction_id: TransactionId) {
        self.xmax = Some(transaction_id);
    }

    pub fn visible_to<F>(&self, snapshot: &Snapshot, reader: TransactionId, mut state_of: F) -> bool
    where
        F: FnMut(TransactionId) -> Option<TransactionState>,
    {
        let creator_state = state_of(self.xmin).unwrap_or(TransactionState::Aborted);

        if !transaction_visible(self.xmin, creator_state, snapshot, reader) {
            return false;
        }

        let Some(xmax) = self.xmax else {
            return true;
        };

        if xmax == reader {
            return false;
        }

        let Some(deleter_state) = state_of(xmax) else {
            return true;
        };

        if deleter_state == TransactionState::Aborted {
            return true;
        }

        !transaction_visible(xmax, deleter_state, snapshot, reader)
    }
}
