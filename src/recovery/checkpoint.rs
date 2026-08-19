use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::transaction::{TransactionId, TransactionState};

use super::Lsn;

const MAGIC: u32 = 0x4350_544b;
const VERSION: u32 = 2;

const HEADER_SIZE: usize = 32;
const STATE_SIZE: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    lsn: Lsn,
    next_transaction_id: TransactionId,
    states: HashMap<TransactionId, TransactionState>,
}

impl Checkpoint {
    pub fn new(
        lsn: Lsn,
        next_transaction_id: TransactionId,
        states: HashMap<TransactionId, TransactionState>,
    ) -> Self {
        Self {
            lsn,
            next_transaction_id,
            states,
        }
    }

    pub fn lsn(&self) -> Lsn {
        self.lsn
    }

    pub fn next_transaction_id(&self) -> TransactionId {
        self.next_transaction_id
    }

    pub fn states(&self) -> &HashMap<TransactionId, TransactionState> {
        &self.states
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

        let mut header = [0_u8; HEADER_SIZE];

        file.read_exact(&mut header)?;

        let magic = u32::from_le_bytes(header[0..4].try_into().expect("fixed checkpoint magic"));

        let version =
            u32::from_le_bytes(header[4..8].try_into().expect("fixed checkpoint version"));

        if magic != MAGIC || version != VERSION {
            return Err(CheckpointError::Corrupt);
        }

        let lsn = u64::from_le_bytes(header[8..16].try_into().expect("fixed checkpoint LSN"));

        let next_transaction_id = u64::from_le_bytes(
            header[16..24]
                .try_into()
                .expect("fixed next transaction id"),
        );

        let state_count = u64::from_le_bytes(
            header[24..32]
                .try_into()
                .expect("fixed checkpoint state count"),
        );

        let mut states = HashMap::new();

        for _ in 0..state_count {
            let mut bytes = [0_u8; STATE_SIZE];

            file.read_exact(&mut bytes)?;

            let transaction_id =
                u64::from_le_bytes(bytes[0..8].try_into().expect("fixed transaction id"));

            let state = decode_state(bytes[8])?;

            states.insert(transaction_id, state);
        }

        Ok(Some(Checkpoint::new(lsn, next_transaction_id, states)))
    }

    pub fn store(&self, checkpoint: &Checkpoint) -> Result<(), CheckpointError> {
        let temporary = self.path.with_extension("checkpoint.tmp");

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temporary)?;

        file.write_all(&MAGIC.to_le_bytes())?;

        file.write_all(&VERSION.to_le_bytes())?;

        file.write_all(&checkpoint.lsn().to_le_bytes())?;

        file.write_all(&checkpoint.next_transaction_id().to_le_bytes())?;

        file.write_all(&(checkpoint.states().len() as u64).to_le_bytes())?;

        let mut states: Vec<_> = checkpoint.states().iter().collect();

        /*
         * Deterministic ordering makes checkpoint files
         * reproducible and easier to inspect/test.
         */
        states.sort_by_key(|(transaction_id, _)| **transaction_id);

        for (transaction_id, state) in states {
            file.write_all(&transaction_id.to_le_bytes())?;

            file.write_all(&[encode_state(*state)])?;

            file.write_all(&[0_u8; 7])?;
        }

        file.sync_all()?;

        std::fs::rename(temporary, &self.path)?;

        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn encode_state(state: TransactionState) -> u8 {
    match state {
        TransactionState::Active => 1,
        TransactionState::Committed => 2,
        TransactionState::Aborted => 3,
    }
}

fn decode_state(value: u8) -> Result<TransactionState, CheckpointError> {
    match value {
        1 => Ok(TransactionState::Active),
        2 => Ok(TransactionState::Committed),
        3 => Ok(TransactionState::Aborted),
        _ => Err(CheckpointError::Corrupt),
    }
}
