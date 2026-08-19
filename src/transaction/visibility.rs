use super::{Snapshot, TransactionId, TransactionState};

pub fn transaction_visible(
    creator: TransactionId,

    creator_state: TransactionState,

    snapshot: &Snapshot,

    reader: TransactionId,
) -> bool {
    if creator == reader {
        return creator_state != TransactionState::Aborted;
    }

    if creator_state != TransactionState::Committed {
        return false;
    }

    if snapshot.started_after_snapshot(creator) {
        return false;
    }

    if snapshot.was_active(creator) {
        return false;
    }

    true
}
