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

    pub fn reclaimable<F>(&self, oldest_active_xmin: TransactionId, mut state_of: F) -> bool
    where
        F: FnMut(TransactionId) -> Option<TransactionState>,
    {
        /*
         * An aborted creator can never become visible again.
         */
        if matches!(state_of(self.xmin), Some(TransactionState::Aborted)) {
            return true;
        }

        /*
         * A version deleted by a committed transaction is safe
         * to reclaim only when that transaction precedes the
         * oldest snapshot that can still be active.
         *
         * Active/aborted/unknown deleters cannot make the
         * version garbage.
         */
        let Some(xmax) = self.xmax else {
            return false;
        };

        matches!(state_of(xmax), Some(TransactionState::Committed)) && xmax < oldest_active_xmin
    }

    pub fn conflicting_writer<F>(
        &self,
        writer: TransactionId,
        mut state_of: F,
    ) -> Option<TransactionId>
    where
        F: FnMut(TransactionId) -> Option<TransactionState>,
    {
        let xmax = self.xmax?;

        if xmax == writer {
            return None;
        }

        match state_of(xmax) {
            Some(TransactionState::Active) | Some(TransactionState::Committed) => Some(xmax),

            Some(TransactionState::Aborted) | None => None,
        }
    }
}
