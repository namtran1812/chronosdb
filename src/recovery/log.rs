use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::PageId;

pub type Lsn = u64;

const HEADER_SIZE: usize = 36;
const MAGIC: u32 = 0x4348_524f;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LogRecordType {
    PageWrite = 1,
    TransactionBegin = 2,
    TransactionCommit = 3,
    TransactionAbort = 4,
}

impl TryFrom<u8> for LogRecordType {
    type Error = std::io::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::PageWrite),
            2 => Ok(Self::TransactionBegin),
            3 => Ok(Self::TransactionCommit),
            4 => Ok(Self::TransactionAbort),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unknown WAL record type",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRecord {
    pub lsn: Lsn,
    pub record_type: LogRecordType,
    pub page_id: PageId,
    pub offset: u32,
    pub payload: Vec<u8>,
}

impl LogRecord {
    pub fn transaction(lsn: Lsn, transaction_id: u64, record_type: LogRecordType) -> Self {
        debug_assert!(matches!(
            record_type,
            LogRecordType::TransactionBegin
                | LogRecordType::TransactionCommit
                | LogRecordType::TransactionAbort
        ));

        Self {
            lsn,
            record_type,
            page_id: transaction_id,
            offset: 0,
            payload: Vec::new(),
        }
    }

    pub fn transaction_id(&self) -> Option<u64> {
        match self.record_type {
            LogRecordType::TransactionBegin
            | LogRecordType::TransactionCommit
            | LogRecordType::TransactionAbort => Some(self.page_id),
            LogRecordType::PageWrite => None,
        }
    }

    pub fn page_write(lsn: Lsn, page_id: PageId, offset: u32, payload: Vec<u8>) -> Self {
        Self {
            lsn,
            record_type: (LogRecordType::PageWrite),
            page_id,
            offset,
            payload,
        }
    }

    fn encoded_len(&self) -> usize {
        HEADER_SIZE + self.payload.len()
    }

    fn encode(&self) -> Vec<u8> {
        let payload_len = self.payload.len() as u32;

        let total_len = self.encoded_len() as u32;

        let mut bytes = Vec::with_capacity(total_len as usize);

        bytes.extend_from_slice(&MAGIC.to_le_bytes());

        bytes.extend_from_slice(&total_len.to_le_bytes());

        bytes.extend_from_slice(&self.lsn.to_le_bytes());

        bytes.push(self.record_type as u8);

        bytes.extend_from_slice(&[0; 3]);

        bytes.extend_from_slice(&self.page_id.to_le_bytes());

        bytes.extend_from_slice(&self.offset.to_le_bytes());

        bytes.extend_from_slice(&payload_len.to_le_bytes());

        bytes.extend_from_slice(&self.payload);

        bytes
    }

    fn decode(bytes: &[u8]) -> std::io::Result<Self> {
        if bytes.len() < HEADER_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "truncated WAL record",
            ));
        }

        let magic = u32::from_le_bytes(bytes[0..4].try_into().expect("fixed-width WAL magic"));

        if magic != MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid WAL magic",
            ));
        }

        let total_len =
            u32::from_le_bytes(bytes[4..8].try_into().expect("fixed-width WAL length")) as usize;

        if total_len != bytes.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "WAL record length mismatch",
            ));
        }

        let lsn = u64::from_le_bytes(bytes[8..16].try_into().expect("fixed-width WAL LSN"));

        let record_type = LogRecordType::try_from(bytes[16])?;

        let page_id =
            u64::from_le_bytes(bytes[20..28].try_into().expect("fixed-width WAL page id"));

        let offset = u32::from_le_bytes(bytes[28..32].try_into().expect("fixed-width WAL offset"));

        let payload_len = u32::from_le_bytes(
            bytes[32..36]
                .try_into()
                .expect("fixed-width WAL payload length"),
        ) as usize;

        if HEADER_SIZE + payload_len != bytes.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "WAL payload length mismatch",
            ));
        }

        let payload = bytes[HEADER_SIZE..].to_vec();

        Ok(Self {
            lsn,
            record_type,
            page_id,
            offset,
            payload,
        })
    }
}

pub struct LogManager {
    file: File,
    path: PathBuf,
    next_lsn: Lsn,
    durable_lsn: Option<Lsn>,
}

impl LogManager {
    pub fn open(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();

        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)?;

        let records = read_valid_records(&mut file)?;

        let physical_next = records.last().map_or(0, |record| record.lsn + 1);

        let persisted_next = read_next_lsn_metadata(&path)?;

        let next_lsn = physical_next.max(persisted_next.unwrap_or(0));

        let durable_lsn = records.last().map(|record| record.lsn);

        file.seek(SeekFrom::End(0))?;

        Ok(Self {
            file,
            path,
            next_lsn,
            durable_lsn,
        })
    }

    pub fn append_page_write(
        &mut self,
        page_id: PageId,
        offset: u32,
        payload: &[u8],
    ) -> std::io::Result<Lsn> {
        let lsn = self.next_lsn;

        self.next_lsn += 1;

        let record = LogRecord::page_write(lsn, page_id, offset, payload.to_vec());

        let bytes = record.encode();

        self.file.write_all(&bytes)?;

        Ok(lsn)
    }

    pub fn append_transaction_begin(&mut self, transaction_id: u64) -> std::io::Result<Lsn> {
        self.append_transaction_record(transaction_id, LogRecordType::TransactionBegin)
    }

    pub fn append_transaction_commit(&mut self, transaction_id: u64) -> std::io::Result<Lsn> {
        self.append_transaction_record(transaction_id, LogRecordType::TransactionCommit)
    }

    pub fn append_transaction_abort(&mut self, transaction_id: u64) -> std::io::Result<Lsn> {
        self.append_transaction_record(transaction_id, LogRecordType::TransactionAbort)
    }

    fn append_transaction_record(
        &mut self,
        transaction_id: u64,
        record_type: LogRecordType,
    ) -> std::io::Result<Lsn> {
        let lsn = self.next_lsn;
        self.next_lsn += 1;

        let record = LogRecord::transaction(lsn, transaction_id, record_type);

        self.file.write_all(&record.encode())?;

        Ok(lsn)
    }

    pub fn flush(&mut self) -> std::io::Result<()> {
        self.file.sync_data()?;

        if self.next_lsn > 0 {
            self.durable_lsn = Some(self.next_lsn - 1);
        }

        persist_next_lsn_metadata(&self.path, self.next_lsn)?;

        Ok(())
    }

    pub fn durable_lsn(&self) -> Option<Lsn> {
        self.durable_lsn
    }

    pub fn next_lsn(&self) -> Lsn {
        self.next_lsn
    }

    pub fn records(&mut self) -> std::io::Result<Vec<LogRecord>> {
        read_valid_records(&mut self.file)
    }

    pub fn records_after(&mut self, lsn: Lsn) -> std::io::Result<Vec<LogRecord>> {
        Ok(read_valid_records(&mut self.file)?
            .into_iter()
            .filter(|record| record.lsn > lsn)
            .collect())
    }

    pub fn compact_through(&mut self, checkpoint_lsn: Lsn) -> std::io::Result<usize> {
        /*
         * Ensure everything currently appended is durable
         * before rewriting the WAL.
         */
        self.flush()?;

        let records = read_valid_records(&mut self.file)?;

        let retained: Vec<_> = records
            .into_iter()
            .filter(|record| record.lsn > checkpoint_lsn)
            .collect();

        let temporary = self.path.with_extension("wal.compact.tmp");

        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&temporary)?;

            for record in &retained {
                file.write_all(&record.encode())?;
            }

            file.sync_all()?;
        }

        std::fs::rename(&temporary, &self.path)?;

        self.file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&self.path)?;

        self.file.seek(SeekFrom::End(0))?;

        /*
         * Do NOT derive next_lsn from retained records.
         * It must remain logically monotonic across compaction.
         */
        persist_next_lsn_metadata(&self.path, self.next_lsn)?;

        self.durable_lsn = self.next_lsn.checked_sub(1);

        Ok(retained.len())
    }

    /// Forces the WAL to stable storage through at least `lsn`.
    ///
    /// This is the write-ahead half of the WAL-before-data invariant:
    /// a page carrying `lsn` must not reach durable storage before the
    /// corresponding log record is durable.
    pub fn flush_through(&mut self, lsn: Lsn) -> std::io::Result<()> {
        if lsn >= self.next_lsn {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cannot flush through unknown LSN",
            ));
        }

        if self.durable_lsn.is_some_and(|durable| durable >= lsn) {
            return Ok(());
        }

        self.flush()
    }
}

fn read_valid_records(file: &mut File) -> std::io::Result<Vec<LogRecord>> {
    file.seek(SeekFrom::Start(0))?;

    let mut records = Vec::new();

    loop {
        let mut prefix = [0_u8; 8];

        match file.read_exact(&mut prefix) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(error) => {
                return Err(error);
            }
        }

        let magic = u32::from_le_bytes(prefix[0..4].try_into().expect("fixed-width WAL magic"));

        if magic != MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid WAL magic",
            ));
        }

        let total_len =
            u32::from_le_bytes(prefix[4..8].try_into().expect("fixed-width WAL length")) as usize;

        if total_len < HEADER_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid WAL record length",
            ));
        }

        let mut bytes = vec![0_u8; total_len];

        bytes[0..8].copy_from_slice(&prefix);

        if let Err(error) = file.read_exact(&mut bytes[8..]) {
            if error.kind() == std::io::ErrorKind::UnexpectedEof {
                break;
            }

            return Err(error);
        }

        records.push(LogRecord::decode(&bytes)?);
    }

    file.seek(SeekFrom::End(0))?;

    Ok(records)
}

fn wal_metadata_path(wal_path: &Path) -> PathBuf {
    let mut name = wal_path.as_os_str().to_os_string();

    name.push(".meta");

    PathBuf::from(name)
}

fn read_next_lsn_metadata(wal_path: &Path) -> std::io::Result<Option<Lsn>> {
    let path = wal_metadata_path(wal_path);

    if !path.exists() {
        return Ok(None);
    }

    let mut file = File::open(path)?;

    let mut bytes = [0_u8; 8];

    file.read_exact(&mut bytes)?;

    Ok(Some(Lsn::from_le_bytes(bytes)))
}

fn persist_next_lsn_metadata(wal_path: &Path, next_lsn: Lsn) -> std::io::Result<()> {
    let path = wal_metadata_path(wal_path);

    let temporary = path.with_extension("meta.tmp");

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&temporary)?;

    file.write_all(&next_lsn.to_le_bytes())?;

    file.sync_all()?;

    std::fs::rename(temporary, path)?;

    Ok(())
}
