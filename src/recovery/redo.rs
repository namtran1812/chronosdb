use crate::PAGE_SIZE;
use crate::storage::{DiskManager, PageError};

use super::{LogManager, LogRecordType};

#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error("disk I/O failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("page write exceeds page boundary")]
    PageBoundary,

    #[error("page mutation failed: {0}")]
    Page(#[from] PageError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryStats {
    pub records_seen: usize,
    pub records_redone: usize,
}

pub struct RecoveryManager;

impl RecoveryManager {
    pub fn redo(
        log: &mut LogManager,
        disk: &mut DiskManager,
    ) -> Result<RecoveryStats, RecoveryError> {
        let records = log.records()?;

        let mut stats = RecoveryStats {
            records_seen: records.len(),
            records_redone: 0,
        };

        for record in records {
            match record.record_type {
                LogRecordType::PageWrite => {
                    let end = record.offset as usize + record.payload.len();

                    if end > PAGE_SIZE {
                        return Err(RecoveryError::PageBoundary);
                    }

                    while disk.page_count() <= record.page_id {
                        disk.allocate_page()?;
                    }

                    let mut page = disk.read_page(record.page_id)?;

                    if page
                        .page_lsn()
                        .is_some_and(|page_lsn| page_lsn >= record.lsn)
                    {
                        continue;
                    }

                    page.write(record.offset as usize, &record.payload)?;

                    page.set_page_lsn(record.lsn);

                    disk.write_page(&page)?;

                    stats.records_redone += 1;
                }
                LogRecordType::TransactionBegin
                | LogRecordType::TransactionCommit
                | LogRecordType::TransactionAbort => {}
            }
        }

        disk.sync()?;

        Ok(stats)
    }
}
