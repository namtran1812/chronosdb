use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::storage::Page;
use crate::{PageId, PAGE_SIZE};

pub struct DiskManager {
    file: File,
    next_page_id: PageId,
}

impl DiskManager {
    pub fn open(
        path: impl AsRef<Path>,
    ) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;

        let length = file.metadata()?.len();

        if length % PAGE_SIZE as u64 != 0 {
            return Err(
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "database file is not page aligned",
                ),
            );
        }

        Ok(Self {
            file,
            next_page_id: (
                length / PAGE_SIZE as u64
            ),
        })
    }

    pub fn allocate_page(
        &mut self,
    ) -> std::io::Result<Page> {
        let id = self.next_page_id;

        self.next_page_id += 1;

        let page = Page::new(id);

        self.write_page(&page)?;

        Ok(page)
    }

    pub fn write_page(
        &mut self,
        page: &Page,
    ) -> std::io::Result<()> {
        let offset = page
            .id()
            .checked_mul(PAGE_SIZE as u64)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "page offset overflow",
                )
            })?;

        self.file.seek(
            SeekFrom::Start(offset)
        )?;

        self.file.write_all(
            page.data()
        )?;

        Ok(())
    }

    pub fn read_page(
        &mut self,
        id: PageId,
    ) -> std::io::Result<Page> {
        if id >= self.next_page_id {
            return Err(
                std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "page does not exist",
                ),
            );
        }

        let offset = id
            .checked_mul(PAGE_SIZE as u64)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "page offset overflow",
                )
            })?;

        let mut page = Page::new(id);

        self.file.seek(
            SeekFrom::Start(offset)
        )?;

        self.file.read_exact(
            page.data_mut_for_disk(),
        )?;

        page.mark_clean();

        Ok(page)
    }

    pub fn sync(
        &mut self,
    ) -> std::io::Result<()> {
        self.file.sync_data()
    }

    pub fn page_count(&self) -> PageId {
        self.next_page_id
    }
}
