use crate::recovery::Lsn;
use crate::{PAGE_SIZE, PageId};

pub const PAGE_HEADER_SIZE: usize = 16;

const PAGE_LSN_OFFSET: usize = 0;
const PAGE_LSN_SIZE: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    id: PageId,
    data: Box<[u8; PAGE_SIZE]>,
    dirty: bool,
}

impl Page {
    pub fn new(id: PageId) -> Self {
        Self {
            id,
            data: Box::new([0; PAGE_SIZE]),
            dirty: false,
        }
    }

    pub fn id(&self) -> PageId {
        self.id
    }

    pub fn data(&self) -> &[u8; PAGE_SIZE] {
        &self.data
    }

    pub(crate) fn data_mut_for_disk(&mut self) -> &mut [u8; PAGE_SIZE] {
        &mut self.data
    }

    pub fn write(&mut self, offset: usize, bytes: &[u8]) -> Result<(), PageError> {
        let end = offset
            .checked_add(bytes.len())
            .ok_or(PageError::OutOfBounds)?;

        if offset < PAGE_HEADER_SIZE || end > PAGE_SIZE {
            return Err(PageError::OutOfBounds);
        }

        self.data[offset..end].copy_from_slice(bytes);

        self.dirty = true;

        Ok(())
    }

    pub fn read(&self, offset: usize, length: usize) -> Result<&[u8], PageError> {
        let end = offset.checked_add(length).ok_or(PageError::OutOfBounds)?;

        if offset < PAGE_HEADER_SIZE || end > PAGE_SIZE {
            return Err(PageError::OutOfBounds);
        }

        Ok(&self.data[offset..end])
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn page_lsn(&self) -> Option<Lsn> {
        let raw = u64::from_le_bytes(
            self.data[PAGE_LSN_OFFSET..PAGE_LSN_OFFSET + PAGE_LSN_SIZE]
                .try_into()
                .expect("page LSN header has fixed width"),
        );

        raw.checked_sub(1)
    }

    pub fn set_page_lsn(&mut self, lsn: Lsn) {
        let encoded = lsn
            .checked_add(1)
            .expect("LSN exceeds persistent page format");

        self.data[PAGE_LSN_OFFSET..PAGE_LSN_OFFSET + PAGE_LSN_SIZE]
            .copy_from_slice(&encoded.to_le_bytes());

        self.dirty = true;
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PageError {
    #[error("page access is out of bounds")]
    OutOfBounds,
}
