use crate::transaction::{
    Snapshot, TransactionId, TransactionState, TupleCodecError, TupleVersion, decode_tuple,
    encode_tuple,
};

use super::{RecordId, SlotId, SlottedPage, SlottedPageError};

#[derive(Debug, thiserror::Error)]
pub enum MvccPageError {
    #[error("slotted page error: {0}")]
    Slotted(#[from] SlottedPageError),

    #[error("tuple codec error: {0}")]
    Tuple(#[from] TupleCodecError),
}

pub struct MvccPage {
    page_id: u64,
    page: SlottedPage,
}

impl MvccPage {
    pub fn new(page_id: u64) -> Self {
        Self {
            page_id,
            page: SlottedPage::new(),
        }
    }

    pub fn from_slotted(page_id: u64, page: SlottedPage) -> Self {
        Self { page_id, page }
    }

    pub fn into_slotted(self) -> SlottedPage {
        self.page
    }

    pub fn insert_version(&mut self, version: &TupleVersion) -> Result<RecordId, MvccPageError> {
        let bytes = encode_tuple(version);

        let slot_id = self.page.insert(&bytes)?;

        Ok(RecordId::new(self.page_id, slot_id))
    }

    pub fn get_version(&self, slot_id: SlotId) -> Result<TupleVersion, MvccPageError> {
        let bytes = self.page.get(slot_id)?;

        Ok(decode_tuple(bytes)?)
    }

    pub fn visible_versions<F>(
        &self,
        snapshot: &Snapshot,
        reader: TransactionId,
        mut state_of: F,
    ) -> Result<Vec<(RecordId, TupleVersion)>, MvccPageError>
    where
        F: FnMut(TransactionId) -> Option<TransactionState>,
    {
        let mut visible = Vec::new();

        for slot_id in 0..self.page.slot_count() {
            let bytes = match self.page.get(slot_id) {
                Ok(bytes) => bytes,
                Err(SlottedPageError::Deleted) => {
                    continue;
                }
                Err(error) => {
                    return Err(error.into());
                }
            };

            let version = decode_tuple(bytes)?;

            if version.visible_to(snapshot, reader, &mut state_of) {
                visible.push((RecordId::new(self.page_id, slot_id), version));
            }
        }

        Ok(visible)
    }

    pub fn slotted(&self) -> &SlottedPage {
        &self.page
    }
}
