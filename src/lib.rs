pub mod recovery;
pub mod storage;
pub mod transaction;

pub type PageId = u64;

pub const PAGE_SIZE: usize = 4096;
