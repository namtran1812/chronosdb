pub mod disk;
pub mod page;
pub mod slotted;

pub use disk::DiskManager;
pub use page::{Page, PageError};

pub use slotted::{SlotId, SlottedPage, SlottedPageError};
