use crate::storage::{
    BufferPoolError, BufferPoolManager, MvccPage, MvccPageError, RecordId, SlottedPage,
    SlottedPageError,
};
use crate::transaction::{Snapshot, TransactionId, TransactionState, TupleVersion};
use crate::{PAGE_SIZE, PageId};

#[derive(Debug, thiserror::Error)]
pub enum BufferedHeapError {
    #[error("buffer pool error: {0}")]
    Buffer(#[from] BufferPoolError),

    #[error("MVCC page error: {0}")]
    Mvcc(#[from] MvccPageError),

    #[error("tuple does not fit in a heap page")]
    TupleTooLarge,
}

pub struct BufferedHeapFile {
    buffer: BufferPoolManager,
}

impl BufferedHeapFile {
    pub fn new(buffer: BufferPoolManager) -> Self {
        Self { buffer }
    }

    pub fn page_count(&self) -> PageId {
        self.buffer.page_count()
    }

    pub fn insert_version(
        &mut self,
        version: &TupleVersion,
    ) -> Result<RecordId, BufferedHeapError> {
        for page_id in 0..self.page_count() {
            let slotted = self.read_slotted(page_id)?;

            let mut mvcc = MvccPage::from_slotted(page_id, slotted);

            match mvcc.insert_version(version) {
                Ok(record_id) => {
                    self.write_mvcc_page(page_id, mvcc)?;

                    return Ok(record_id);
                }

                Err(MvccPageError::Slotted(SlottedPageError::NoSpace)) => {}

                Err(error) => {
                    return Err(error.into());
                }
            }
        }

        self.insert_into_new_page(version)
    }

    pub fn replace_version(
        &mut self,
        record_id: RecordId,
        version: &TupleVersion,
    ) -> Result<(), BufferedHeapError> {
        let slotted = self.read_slotted(record_id.page_id())?;

        let mut mvcc = MvccPage::from_slotted(record_id.page_id(), slotted);

        mvcc.replace_version(record_id.slot_id(), version)?;

        self.write_mvcc_page(record_id.page_id(), mvcc)?;

        Ok(())
    }

    pub fn get_version(&mut self, record_id: RecordId) -> Result<TupleVersion, BufferedHeapError> {
        let slotted = self.read_slotted(record_id.page_id())?;

        let mvcc = MvccPage::from_slotted(record_id.page_id(), slotted);

        Ok(mvcc.get_version(record_id.slot_id())?)
    }

    pub fn visible_scan<F>(
        &mut self,
        snapshot: &Snapshot,
        reader: TransactionId,
        mut state_of: F,
    ) -> Result<Vec<(RecordId, TupleVersion)>, BufferedHeapError>
    where
        F: FnMut(TransactionId) -> Option<TransactionState>,
    {
        let mut rows = Vec::new();

        for page_id in 0..self.page_count() {
            let slotted = self.read_slotted(page_id)?;

            let mvcc = MvccPage::from_slotted(page_id, slotted);

            let mut visible = mvcc.visible_versions(snapshot, reader, &mut state_of)?;

            rows.append(&mut visible);
        }

        Ok(rows)
    }

    pub fn flush_all(&mut self) -> Result<(), BufferedHeapError> {
        self.buffer.flush_all()?;
        Ok(())
    }

    fn insert_into_new_page(
        &mut self,
        version: &TupleVersion,
    ) -> Result<RecordId, BufferedHeapError> {
        let page_id = self.buffer.new_page()?;

        let mut mvcc = MvccPage::new(page_id);

        let record_id = match mvcc.insert_version(version) {
            Ok(record_id) => record_id,

            Err(MvccPageError::Slotted(SlottedPageError::NoSpace)) => {
                self.buffer.unpin_page(page_id, false)?;

                return Err(BufferedHeapError::TupleTooLarge);
            }

            Err(error) => {
                self.buffer.unpin_page(page_id, false)?;

                return Err(error.into());
            }
        };

        self.write_mvcc_page(page_id, mvcc)?;

        self.buffer.unpin_page(page_id, true)?;

        Ok(record_id)
    }

    fn read_slotted(&mut self, page_id: PageId) -> Result<SlottedPage, BufferedHeapError> {
        let bytes = {
            let page = self.buffer.fetch_page(page_id)?;

            let mut bytes = Box::new([0_u8; PAGE_SIZE]);

            bytes.copy_from_slice(page.data());

            bytes
        };

        self.buffer.unpin_page(page_id, false)?;

        Ok(SlottedPage::from_bytes(bytes))
    }

    fn write_mvcc_page(
        &mut self,
        page_id: PageId,
        mvcc: MvccPage,
    ) -> Result<(), BufferedHeapError> {
        let slotted = mvcc.into_slotted();

        let bytes = slotted.as_bytes();

        self.buffer.logged_write(
            page_id,
            crate::storage::PAGE_HEADER_SIZE,
            &bytes[crate::storage::PAGE_HEADER_SIZE..],
        )?;

        Ok(())
    }
}
