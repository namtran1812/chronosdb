use std::collections::BTreeSet;

use super::TransactionId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    xmin: TransactionId,
    xmax: TransactionId,
    active: BTreeSet<TransactionId>,
}

impl Snapshot {
    pub fn new(xmin: TransactionId, xmax: TransactionId, active: BTreeSet<TransactionId>) -> Self {
        Self { xmin, xmax, active }
    }

    pub fn xmin(&self) -> TransactionId {
        self.xmin
    }

    pub fn xmax(&self) -> TransactionId {
        self.xmax
    }

    pub fn active_transactions(&self) -> &BTreeSet<TransactionId> {
        &self.active
    }

    pub fn was_active(&self, transaction_id: TransactionId) -> bool {
        self.active.contains(&transaction_id)
    }

    pub fn started_after_snapshot(&self, transaction_id: TransactionId) -> bool {
        transaction_id >= self.xmax
    }
}
