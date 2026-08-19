use crate::PageId;

use super::SlotId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecordId {
    page_id: PageId,
    slot_id: SlotId,
}

impl RecordId {
    pub fn new(page_id: PageId, slot_id: SlotId) -> Self {
        Self { page_id, slot_id }
    }

    pub fn page_id(&self) -> PageId {
        self.page_id
    }

    pub fn slot_id(&self) -> SlotId {
        self.slot_id
    }
}
