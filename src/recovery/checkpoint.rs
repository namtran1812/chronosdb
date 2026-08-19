use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::Lsn;

const MAGIC: u32 = 0x4350_544b;
const VERSION: u32 = 1;
const FILE_SIZE: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint {
    lsn: Lsn,
}

impl Checkpoint {
    pub fn new(lsn: Lsn) -> Self {
        Self { lsn }
    }

    pub fn lsn(&self) -> Lsn {
        self.lsn
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CheckpointError {
    #[error("checkpoint I/O failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("checkpoint file is corrupted")]
    Corrupt,
}

pub struct CheckpointManager {
    path: PathBuf,
}

impl CheckpointManager {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn load(&self) -> Result<Option<Checkpoint>, CheckpointError> {
        if !self.path.exists() {
            return Ok(None);
        }

        let mut file = File::open(&self.path)?;

        let mut bytes = [0_u8; FILE_SIZE];

        file.read_exact(&mut bytes)?;

        let magic = u32::from_le_bytes(
            bytes[0..4]
                .try_into()
                .expect("fixed-width checkpoint magic"),
        );

        let version = u32::from_le_bytes(
            bytes[4..8]
                .try_into()
                .expect("fixed-width checkpoint version"),
        );

        if magic != MAGIC || version != VERSION {
            return Err(CheckpointError::Corrupt);
        }

        let lsn = u64::from_le_bytes(bytes[8..16].try_into().expect("fixed-width checkpoint LSN"));

        Ok(Some(Checkpoint::new(lsn)))
    }

    pub fn store(&self, checkpoint: Checkpoint) -> Result<(), CheckpointError> {
        let temporary = self.path.with_extension("checkpoint.tmp");

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temporary)?;

        file.write_all(&MAGIC.to_le_bytes())?;

        file.write_all(&VERSION.to_le_bytes())?;

        file.write_all(&checkpoint.lsn().to_le_bytes())?;

        file.sync_all()?;

        std::fs::rename(temporary, &self.path)?;

        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
