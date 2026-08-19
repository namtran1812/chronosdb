use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::PageId;
use crate::recovery::{LogManager, Lsn};
use crate::storage::{DiskManager, Page, PageError};

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

    #[error("logged page write requires a WAL")]
    WalNotConfigured,

    #[error("page mutation failed: {0}")]
    Page(String),

    #[error("disk I/O failed: {0}")]
    Io(String),
}

impl From<std::io::Error> for BufferPoolError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl From<PageError> for BufferPoolError {
    fn from(error: PageError) -> Self {
        Self::Page(error.to_string())
    }
}

pub struct BufferPoolManager {
    disk: DiskManager,
    wal: Option<Rc<RefCell<LogManager>>>,
    frames: Vec<Frame>,
    page_table: HashMap<PageId, FrameId>,
    clock: u64,
}

impl BufferPoolManager {
    pub fn new(disk: DiskManager, pool_size: usize) -> Self {
        Self::build(disk, None, pool_size)
    }

    pub fn new_with_wal(disk: DiskManager, wal: LogManager, pool_size: usize) -> Self {
        Self::build(disk, Some(Rc::new(RefCell::new(wal))), pool_size)
    }

    pub fn new_with_shared_wal(
        disk: DiskManager,
        wal: Rc<RefCell<LogManager>>,
        pool_size: usize,
    ) -> Self {
        Self::build(disk, Some(wal), pool_size)
    }

    fn build(disk: DiskManager, wal: Option<Rc<RefCell<LogManager>>>, pool_size: usize) -> Self {
        assert!(pool_size > 0, "buffer pool size must be positive");

        Self {
            disk,
            wal,
            frames: (0..pool_size).map(|_| Frame::empty()).collect(),
            page_table: HashMap::new(),
            clock: 0,
        }
    }

    pub fn pool_size(&self) -> usize {
        self.frames.len()
    }

    pub fn page_count(&self) -> PageId {
        self.disk.page_count()
    }

    pub fn resident_pages(&self) -> usize {
        self.page_table.len()
    }

    pub fn wal_durable_lsn(&self) -> Option<Lsn> {
        self.wal.as_ref().and_then(|wal| wal.borrow().durable_lsn())
    }

    pub fn flush_wal_through(&mut self, lsn: Lsn) -> Result<(), BufferPoolError> {
        let wal = self.wal.as_ref().ok_or(BufferPoolError::WalNotConfigured)?;

        wal.borrow_mut().flush_through(lsn)?;

        Ok(())
    }

    pub fn new_page(&mut self) -> Result<PageId, BufferPoolError> {
        let frame_id = self.acquire_frame()?;

        let page = self.disk.allocate_page()?;

        let page_id = page.id();

        self.install_page(frame_id, page);

        Ok(page_id)
    }

    pub fn fetch_page(&mut self, page_id: PageId) -> Result<&Page, BufferPoolError> {
        let frame_id = self.ensure_resident(page_id)?;

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
        let frame_id = self.ensure_resident(page_id)?;

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

    pub fn logged_write(
        &mut self,
        page_id: PageId,
        offset: usize,
        bytes: &[u8],
    ) -> Result<Lsn, BufferPoolError> {
        let frame_id = self.ensure_resident(page_id)?;

        let offset_u32 = u32::try_from(offset)
            .map_err(|_| BufferPoolError::Page("page offset exceeds WAL format".to_owned()))?;

        let lsn = {
            let wal = self.wal.as_ref().ok_or(BufferPoolError::WalNotConfigured)?;

            wal.borrow_mut()
                .append_page_write(page_id, offset_u32, bytes)?
        };

        let frame = &mut self.frames[frame_id];

        let page = frame
            .page
            .as_mut()
            .expect("resident frame must contain a page");

        page.write(offset, bytes)?;

        page.set_page_lsn(lsn);

        frame.dirty = true;

        self.clock += 1;
        frame.last_access = self.clock;

        Ok(lsn)
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

        if !self.frame_needs_flush(frame_id) {
            return Ok(());
        }

        self.flush_frame_wal(frame_id)?;

        let frame = &mut self.frames[frame_id];

        let page = frame
            .page
            .as_ref()
            .expect("resident frame must contain a page");

        self.disk.write_page(page)?;

        self.disk.sync()?;

        frame.dirty = false;

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

    pub fn page_lsn(&self, page_id: PageId) -> Option<Lsn> {
        let frame_id = *self.page_table.get(&page_id)?;

        self.frames[frame_id].page.as_ref().and_then(Page::page_lsn)
    }

    pub fn is_resident(&self, page_id: PageId) -> bool {
        self.page_table.contains_key(&page_id)
    }

    fn ensure_resident(&mut self, page_id: PageId) -> Result<FrameId, BufferPoolError> {
        if let Some(&frame_id) = self.page_table.get(&page_id) {
            return Ok(frame_id);
        }

        let frame_id = self.acquire_frame()?;

        let page = self.disk.read_page(page_id)?;

        self.install_page(frame_id, page);

        // Loading a page into the
        // cache must not pin it.
        self.frames[frame_id].pin_count = 0;

        Ok(frame_id)
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
        let page_id = self.frames[frame_id]
            .page
            .as_ref()
            .expect("victim frame must contain a page")
            .id();

        if self.frame_needs_flush(frame_id) {
            self.flush_frame_wal(frame_id)?;

            let page = self.frames[frame_id]
                .page
                .as_ref()
                .expect("victim frame must contain a page");

            self.disk.write_page(page)?;

            self.disk.sync()?;
        }

        self.page_table.remove(&page_id);

        self.frames[frame_id] = Frame::empty();

        Ok(())
    }

    fn frame_needs_flush(&self, frame_id: FrameId) -> bool {
        let frame = &self.frames[frame_id];

        frame.dirty || frame.page.as_ref().is_some_and(Page::is_dirty)
    }

    fn flush_frame_wal(&mut self, frame_id: FrameId) -> Result<(), BufferPoolError> {
        let page_lsn = self.frames[frame_id].page.as_ref().and_then(Page::page_lsn);

        let Some(lsn) = page_lsn else {
            return Ok(());
        };

        let wal = self.wal.as_ref().ok_or(BufferPoolError::WalNotConfigured)?;

        wal.borrow_mut().flush_through(lsn)?;

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
