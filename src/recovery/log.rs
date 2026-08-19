use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::PageId;

pub type Lsn = u64;

const HEADER_SIZE: usize = 36;
const MAGIC: u32 = 0x4348_524f;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LogRecordType {
    PageWrite = 1,
}

impl TryFrom<u8> for LogRecordType {
    type Error = std::io::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::PageWrite),
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
    next_lsn: Lsn,
    durable_lsn: Option<Lsn>,
}

impl LogManager {
    pub fn open(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;

        let records = read_valid_records(&mut file)?;

        let next_lsn = records.last().map_or(0, |record| record.lsn + 1);

        let durable_lsn = records.last().map(|record| record.lsn);

        file.seek(SeekFrom::End(0))?;

        Ok(Self {
            file,
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

    pub fn flush(&mut self) -> std::io::Result<()> {
        self.file.sync_data()?;

        if self.next_lsn > 0 {
            self.durable_lsn = Some(self.next_lsn - 1);
        }

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
