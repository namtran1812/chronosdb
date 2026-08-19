use std::path::{Path, PathBuf};

use crate::storage::{
    DiskManager, MvccPage, MvccPageError, PageError, RecordId, SlottedPage, SlottedPageError,
};
use crate::transaction::{Snapshot, TransactionId, TransactionState, TupleVersion};
use crate::{PAGE_SIZE, PageId};

#[derive(Debug, thiserror::Error)]
pub enum HeapFileError {
    #[error("disk I/O failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("MVCC page error: {0}")]
    Mvcc(#[from] MvccPageError),

    #[error("page mutation failed: {0}")]
    Page(#[from] PageError),

    #[error("tuple does not fit in a heap page")]
    TupleTooLarge,
}

pub struct HeapFile {
    path: PathBuf,
    disk: DiskManager,
}

impl HeapFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, HeapFileError> {
        let path = path.as_ref().to_path_buf();

        let disk = DiskManager::open(&path)?;

        Ok(Self { path, disk })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn page_count(&self) -> PageId {
        self.disk.page_count()
    }

    pub fn insert_version(&mut self, version: &TupleVersion) -> Result<RecordId, HeapFileError> {
        for page_id in 0..self.disk.page_count() {
            let page = self.disk.read_page(page_id)?;

            let slotted = self.page_to_slotted(&page);

            let mut mvcc = MvccPage::from_slotted(page_id, slotted);

            match mvcc.insert_version(version) {
                Ok(record_id) => {
                    self.persist_mvcc_page(page_id, mvcc)?;

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

    pub fn get_version(&mut self, record_id: RecordId) -> Result<TupleVersion, HeapFileError> {
        let page = self.disk.read_page(record_id.page_id())?;

        let slotted = self.page_to_slotted(&page);

        let mvcc = MvccPage::from_slotted(record_id.page_id(), slotted);

        Ok(mvcc.get_version(record_id.slot_id())?)
    }

    pub fn visible_scan<F>(
        &mut self,
        snapshot: &Snapshot,
        reader: TransactionId,
        mut state_of: F,
    ) -> Result<Vec<(RecordId, TupleVersion)>, HeapFileError>
    where
        F: FnMut(TransactionId) -> Option<TransactionState>,
    {
        let mut rows = Vec::new();

        for page_id in 0..self.disk.page_count() {
            let page = self.disk.read_page(page_id)?;

            let slotted = self.page_to_slotted(&page);

            let mvcc = MvccPage::from_slotted(page_id, slotted);

            let mut visible = mvcc.visible_versions(snapshot, reader, &mut state_of)?;

            rows.append(&mut visible);
        }

        Ok(rows)
    }

    pub fn sync(&mut self) -> Result<(), HeapFileError> {
        self.disk.sync()?;

        Ok(())
    }

    fn insert_into_new_page(&mut self, version: &TupleVersion) -> Result<RecordId, HeapFileError> {
        let page = self.disk.allocate_page()?;

        let page_id = page.id();

        let slotted = SlottedPage::new();

        let mut mvcc = MvccPage::from_slotted(page_id, slotted);

        let record_id = match mvcc.insert_version(version) {
            Ok(record_id) => record_id,

            Err(MvccPageError::Slotted(SlottedPageError::NoSpace)) => {
                return Err(HeapFileError::TupleTooLarge);
            }

            Err(error) => {
                return Err(error.into());
            }
        };

        self.persist_mvcc_page(page_id, mvcc)?;

        Ok(record_id)
    }

    fn persist_mvcc_page(&mut self, page_id: PageId, mvcc: MvccPage) -> Result<(), HeapFileError> {
        let slotted = mvcc.into_slotted();

        let mut page = self.disk.read_page(page_id)?;

        let bytes = slotted.as_bytes();

        page.write(
            crate::storage::PAGE_HEADER_SIZE,
            &bytes[crate::storage::PAGE_HEADER_SIZE..],
        )?;

        self.disk.write_page(&page)?;

        Ok(())
    }

    fn page_to_slotted(&self, page: &crate::storage::Page) -> SlottedPage {
        let mut bytes = Box::new([0_u8; PAGE_SIZE]);

        bytes.copy_from_slice(page.data());

        SlottedPage::from_bytes(bytes)
    }
}
