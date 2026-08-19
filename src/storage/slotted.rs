use crate::PAGE_SIZE;
use crate::storage::PAGE_HEADER_SIZE;

pub type SlotId = u16;

const SLOT_HEADER_SIZE: usize = 6;
const HEADER_SIZE: usize = PAGE_HEADER_SIZE + SLOT_HEADER_SIZE;
const SLOT_SIZE: usize = 4;

const SLOT_COUNT_OFFSET: usize = PAGE_HEADER_SIZE;
const FREE_START_OFFSET: usize = PAGE_HEADER_SIZE + 2;
const FREE_END_OFFSET: usize = PAGE_HEADER_SIZE + 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    offset: u16,
    len: u16,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SlottedPageError {
    #[error("record does not fit in page")]
    NoSpace,

    #[error("slot does not exist")]
    InvalidSlot,

    #[error("record is deleted")]
    Deleted,

    #[error("record is too large")]
    RecordTooLarge,
}

pub struct SlottedPage {
    data: Box<[u8; PAGE_SIZE]>,
}

impl Default for SlottedPage {
    fn default() -> Self {
        Self::new()
    }
}

impl SlottedPage {
    pub fn new() -> Self {
        let mut page = Self {
            data: Box::new([0; PAGE_SIZE]),
        };

        page.set_slot_count(0);
        page.set_free_start(HEADER_SIZE as u16);
        page.set_free_end(PAGE_SIZE as u16);

        page
    }

    pub fn from_bytes(data: Box<[u8; PAGE_SIZE]>) -> Self {
        Self { data }
    }

    pub fn as_bytes(&self) -> &[u8; PAGE_SIZE] {
        &self.data
    }

    pub fn slot_count(&self) -> SlotId {
        self.read_u16(SLOT_COUNT_OFFSET)
    }

    pub fn free_space(&self) -> usize {
        let start = self.free_start() as usize;

        let end = self.free_end() as usize;

        end.saturating_sub(start)
    }

    pub fn insert(&mut self, record: &[u8]) -> Result<SlotId, SlottedPageError> {
        if record.len() > u16::MAX as usize {
            return Err(SlottedPageError::RecordTooLarge);
        }

        if let Some(slot_id) = self.find_deleted_slot() {
            if record.len() > self.free_space() {
                self.compact();
            }

            if record.len() > self.free_space() {
                return Err(SlottedPageError::NoSpace);
            }

            let offset = self.allocate_payload(record)?;

            self.write_slot(
                slot_id,
                Slot {
                    offset,
                    len: record.len() as u16,
                },
            );

            return Ok(slot_id);
        }

        let required = record.len() + SLOT_SIZE;

        if required > self.free_space() {
            self.compact();
        }

        if required > self.free_space() {
            return Err(SlottedPageError::NoSpace);
        }

        let slot_id = self.slot_count();

        let offset = self.allocate_payload(record)?;

        self.set_slot_count(slot_id + 1);

        self.set_free_start((HEADER_SIZE + self.slot_count() as usize * SLOT_SIZE) as u16);

        self.write_slot(
            slot_id,
            Slot {
                offset,
                len: record.len() as u16,
            },
        );

        Ok(slot_id)
    }

    pub fn get(&self, slot_id: SlotId) -> Result<&[u8], SlottedPageError> {
        let slot = self.read_slot(slot_id)?;

        if slot.len == 0 {
            return Err(SlottedPageError::Deleted);
        }

        let start = slot.offset as usize;

        let end = start + slot.len as usize;

        if start < HEADER_SIZE || end > PAGE_SIZE {
            return Err(SlottedPageError::InvalidSlot);
        }

        Ok(&self.data[start..end])
    }

    pub fn replace(&mut self, slot_id: SlotId, record: &[u8]) -> Result<(), SlottedPageError> {
        if record.len() > u16::MAX as usize {
            return Err(SlottedPageError::RecordTooLarge);
        }

        let slot = self.read_slot(slot_id)?;

        if slot.len == 0 {
            return Err(SlottedPageError::Deleted);
        }

        /*
         * Shrinking or same-size replacement can preserve
         * the current physical offset immediately.
         */
        if record.len() <= slot.len as usize {
            let start = slot.offset as usize;

            let end = start + record.len();

            self.data[start..end].copy_from_slice(record);

            self.write_slot(
                slot_id,
                Slot {
                    offset: slot.offset,
                    len: record.len() as u16,
                },
            );

            return Ok(());
        }

        /*
         * A growing replacement may reclaim the old tuple's
         * space while keeping the logical SlotId stable.
         */
        let reclaimable = self.free_space() + slot.len as usize;

        if record.len() > reclaimable {
            return Err(SlottedPageError::NoSpace);
        }

        /*
         * Temporarily tombstone the old slot so compaction
         * ignores its current payload.
         */
        self.write_slot(
            slot_id,
            Slot {
                offset: slot.offset,
                len: 0,
            },
        );

        self.compact();

        let offset = self.allocate_payload(record)?;

        self.write_slot(
            slot_id,
            Slot {
                offset,
                len: record.len() as u16,
            },
        );

        Ok(())
    }

    pub fn delete(&mut self, slot_id: SlotId) -> Result<(), SlottedPageError> {
        let mut slot = self.read_slot(slot_id)?;

        if slot.len == 0 {
            return Err(SlottedPageError::Deleted);
        }

        slot.len = 0;

        self.write_slot(slot_id, slot);

        Ok(())
    }

    pub fn compact(&mut self) {
        let slot_count = self.slot_count();

        let mut live = Vec::new();

        for slot_id in 0..slot_count {
            if let Ok(slot) = self.read_slot(slot_id) {
                if slot.len == 0 {
                    continue;
                }

                let start = slot.offset as usize;

                let end = start + slot.len as usize;

                live.push((slot_id, self.data[start..end].to_vec()));
            }
        }

        let mut new_free_end = PAGE_SIZE;

        for (slot_id, bytes) in live {
            new_free_end -= bytes.len();

            self.data[new_free_end..new_free_end + bytes.len()].copy_from_slice(&bytes);

            self.write_slot(
                slot_id,
                Slot {
                    offset: new_free_end as u16,
                    len: bytes.len() as u16,
                },
            );
        }

        self.set_free_end(new_free_end as u16);
    }

    fn allocate_payload(&mut self, record: &[u8]) -> Result<u16, SlottedPageError> {
        let end = self.free_end() as usize;

        if record.len() > end {
            return Err(SlottedPageError::NoSpace);
        }

        let start = end - record.len();

        if start < self.free_start() as usize {
            return Err(SlottedPageError::NoSpace);
        }

        self.data[start..end].copy_from_slice(record);

        self.set_free_end(start as u16);

        Ok(start as u16)
    }

    fn find_deleted_slot(&self) -> Option<SlotId> {
        for slot_id in 0..self.slot_count() {
            if let Ok(slot) = self.read_slot(slot_id)
                && slot.len == 0
            {
                return Some(slot_id);
            }
        }

        None
    }

    fn read_slot(&self, slot_id: SlotId) -> Result<Slot, SlottedPageError> {
        if slot_id >= self.slot_count() {
            return Err(SlottedPageError::InvalidSlot);
        }

        let offset = HEADER_SIZE + slot_id as usize * SLOT_SIZE;

        if offset + SLOT_SIZE > PAGE_SIZE {
            return Err(SlottedPageError::InvalidSlot);
        }

        Ok(Slot {
            offset: self.read_u16(offset),
            len: self.read_u16(offset + 2),
        })
    }

    fn write_slot(&mut self, slot_id: SlotId, slot: Slot) {
        let offset = HEADER_SIZE + slot_id as usize * SLOT_SIZE;

        self.write_u16(offset, slot.offset);

        self.write_u16(offset + 2, slot.len);
    }

    fn free_start(&self) -> u16 {
        self.read_u16(FREE_START_OFFSET)
    }

    fn free_end(&self) -> u16 {
        self.read_u16(FREE_END_OFFSET)
    }

    fn set_slot_count(&mut self, value: u16) {
        self.write_u16(SLOT_COUNT_OFFSET, value);
    }

    fn set_free_start(&mut self, value: u16) {
        self.write_u16(FREE_START_OFFSET, value);
    }

    fn set_free_end(&mut self, value: u16) {
        self.write_u16(FREE_END_OFFSET, value);
    }

    fn read_u16(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.data[offset], self.data[offset + 1]])
    }

    fn write_u16(&mut self, offset: usize, value: u16) {
        let bytes = value.to_le_bytes();

        self.data[offset] = bytes[0];

        self.data[offset + 1] = bytes[1];
    }
}
