use crate::{PageId, PAGE_SIZE};

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

    pub(crate) fn data_mut_for_disk(
        &mut self,
    ) -> &mut [u8; PAGE_SIZE] {
        &mut self.data
    }

    pub fn write(
        &mut self,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), PageError> {
        let end = offset
            .checked_add(bytes.len())
            .ok_or(PageError::OutOfBounds)?;

        if end > PAGE_SIZE {
            return Err(PageError::OutOfBounds);
        }

        self.data[offset..end]
            .copy_from_slice(bytes);

        self.dirty = true;

        Ok(())
    }

    pub fn read(
        &self,
        offset: usize,
        length: usize,
    ) -> Result<&[u8], PageError> {
        let end = offset
            .checked_add(length)
            .ok_or(PageError::OutOfBounds)?;

        if end > PAGE_SIZE {
            return Err(PageError::OutOfBounds);
        }

        Ok(&self.data[offset..end])
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }
}

#[derive(
    Debug,
    thiserror::Error,
    PartialEq,
    Eq,
)]
pub enum PageError {
    #[error("page access is out of bounds")]
    OutOfBounds,
}
