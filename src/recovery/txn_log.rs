use crate::transaction::TransactionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransactionLogKind {
    Begin = 1,
    Commit = 2,
    Abort = 3,
}

impl TryFrom<u8> for TransactionLogKind {
    type Error = std::io::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Begin),
            2 => Ok(Self::Commit),
            3 => Ok(Self::Abort),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unknown transaction WAL record type",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransactionLogRecord {
    pub transaction_id: TransactionId,
    pub kind: TransactionLogKind,
}
