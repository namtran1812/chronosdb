use std::collections::HashMap;

use crate::PageId;
use crate::storage::{DiskManager, Page};

pub type FrameId = usize;

#[derive(Debug)]
struct Frame {
    page: Option<Page>,
    pin_count: usize,
    dirty: bool,
    last_access: u64,
}

impl Frame {
    fn empty() -> Self {
        Self {
            page: None,
            pin_count: 0,
            dirty: false,
            last_access: 0,
        }
    }

    fn is_evictable(&self) -> bool {
        self.page.is_some() && self.pin_count == 0
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BufferPoolError {
    #[error("buffer pool has no evictable frame")]
    NoFrameAvailable,

    #[error("page is not resident in buffer pool")]
    PageNotResident,

    #[error("page pin count is already zero")]
    PinCountUnderflow,

    #[error("disk I/O failed: {0}")]
    Io(String),
}

impl From<std::io::Error> for BufferPoolError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

pub struct BufferPoolManager {
    disk: DiskManager,
    frames: Vec<Frame>,
    page_table: HashMap<PageId, FrameId>,
    clock: u64,
}

impl BufferPoolManager {
    pub fn new(disk: DiskManager, pool_size: usize) -> Self {
        assert!(pool_size > 0, "buffer pool size must be positive");

        Self {
            disk,
            frames: (0..pool_size).map(|_| Frame::empty()).collect(),
            page_table: HashMap::new(),
            clock: 0,
        }
    }

    pub fn pool_size(&self) -> usize {
        self.frames.len()
    }

    pub fn resident_pages(&self) -> usize {
        self.page_table.len()
    }

    pub fn new_page(&mut self) -> Result<PageId, BufferPoolError> {
        let frame_id = self.acquire_frame()?;

        let page = self.disk.allocate_page()?;

        let page_id = page.id();

        self.install_page(frame_id, page);

        Ok(page_id)
    }

    pub fn fetch_page(&mut self, page_id: PageId) -> Result<&Page, BufferPoolError> {
        let frame_id = if let Some(&frame_id) = self.page_table.get(&page_id) {
            frame_id
        } else {
            let frame_id = self.acquire_frame()?;

            let page = self.disk.read_page(page_id)?;

            self.install_page(frame_id, page);

            frame_id
        };

        self.clock += 1;

        let frame = &mut self.frames[frame_id];

        frame.pin_count += 1;
        frame.last_access = self.clock;

        Ok(frame
            .page
            .as_ref()
            .expect("resident frame must contain a page"))
    }

    pub fn fetch_page_mut(&mut self, page_id: PageId) -> Result<&mut Page, BufferPoolError> {
        let frame_id = if let Some(&frame_id) = self.page_table.get(&page_id) {
            frame_id
        } else {
            let frame_id = self.acquire_frame()?;

            let page = self.disk.read_page(page_id)?;

            self.install_page(frame_id, page);

            frame_id
        };

        self.clock += 1;

        let frame = &mut self.frames[frame_id];

        frame.pin_count += 1;
        frame.dirty = true;
        frame.last_access = self.clock;

        Ok(frame
            .page
            .as_mut()
            .expect("resident frame must contain a page"))
    }

    pub fn unpin_page(&mut self, page_id: PageId, dirty: bool) -> Result<(), BufferPoolError> {
        let frame_id = *self
            .page_table
            .get(&page_id)
            .ok_or(BufferPoolError::PageNotResident)?;

        let frame = &mut self.frames[frame_id];

        if frame.pin_count == 0 {
            return Err(BufferPoolError::PinCountUnderflow);
        }

        frame.pin_count -= 1;

        if dirty {
            frame.dirty = true;
        }

        Ok(())
    }

    pub fn flush_page(&mut self, page_id: PageId) -> Result<(), BufferPoolError> {
        let frame_id = *self
            .page_table
            .get(&page_id)
            .ok_or(BufferPoolError::PageNotResident)?;

        let frame = &mut self.frames[frame_id];

        if let Some(page) = frame.page.as_ref()
            && (frame.dirty || page.is_dirty())
        {
            self.disk.write_page(page)?;

            self.disk.sync()?;

            frame.dirty = false;
        }

        Ok(())
    }

    pub fn flush_all(&mut self) -> Result<(), BufferPoolError> {
        let page_ids: Vec<PageId> = self.page_table.keys().copied().collect();

        for page_id in page_ids {
            self.flush_page(page_id)?;
        }

        Ok(())
    }

    pub fn pin_count(&self, page_id: PageId) -> Option<usize> {
        self.page_table
            .get(&page_id)
            .map(|&frame_id| self.frames[frame_id].pin_count)
    }

    pub fn is_resident(&self, page_id: PageId) -> bool {
        self.page_table.contains_key(&page_id)
    }

    fn acquire_frame(&mut self) -> Result<FrameId, BufferPoolError> {
        if let Some(frame_id) = self.frames.iter().position(|frame| frame.page.is_none()) {
            return Ok(frame_id);
        }

        let victim = self
            .frames
            .iter()
            .enumerate()
            .filter(|(_, frame)| frame.is_evictable())
            .min_by_key(|(_, frame)| frame.last_access)
            .map(|(frame_id, _)| frame_id)
            .ok_or(BufferPoolError::NoFrameAvailable)?;

        self.evict_frame(victim)?;

        Ok(victim)
    }

    fn evict_frame(&mut self, frame_id: FrameId) -> Result<(), BufferPoolError> {
        let frame = &mut self.frames[frame_id];

        let page = frame
            .page
            .as_ref()
            .expect("victim frame must contain a page");

        let page_id = page.id();

        if frame.dirty || page.is_dirty() {
            self.disk.write_page(page)?;

            self.disk.sync()?;
        }

        self.page_table.remove(&page_id);

        self.frames[frame_id] = Frame::empty();

        Ok(())
    }

    fn install_page(&mut self, frame_id: FrameId, page: Page) {
        let page_id = page.id();

        self.clock += 1;

        self.frames[frame_id] = Frame {
            page: Some(page),
            pin_count: 1,
            dirty: false,
            last_access: self.clock,
        };

        self.page_table.insert(page_id, frame_id);
    }
}
