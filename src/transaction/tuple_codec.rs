use super::{TransactionId, TupleVersion};

const HEADER_SIZE: usize = 20;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TupleCodecError {
    #[error("tuple record is truncated")]
    Truncated,

    #[error("tuple payload length is invalid")]
    InvalidPayloadLength,
}

pub fn encode_tuple(version: &TupleVersion) -> Vec<u8> {
    let payload = version.payload();

    let payload_len = payload.len() as u32;

    let xmax_encoded = version
        .xmax()
        .and_then(|xmax| xmax.checked_add(1))
        .unwrap_or(0);

    let mut bytes = Vec::with_capacity(HEADER_SIZE + payload.len());

    bytes.extend_from_slice(&version.xmin().to_le_bytes());

    bytes.extend_from_slice(&xmax_encoded.to_le_bytes());

    bytes.extend_from_slice(&payload_len.to_le_bytes());

    bytes.extend_from_slice(payload);

    bytes
}

pub fn decode_tuple(bytes: &[u8]) -> Result<TupleVersion, TupleCodecError> {
    if bytes.len() < HEADER_SIZE {
        return Err(TupleCodecError::Truncated);
    }

    let xmin = TransactionId::from_le_bytes(bytes[0..8].try_into().expect("fixed-width xmin"));

    let xmax_encoded = u64::from_le_bytes(bytes[8..16].try_into().expect("fixed-width xmax"));

    let payload_len = u32::from_le_bytes(
        bytes[16..20]
            .try_into()
            .expect("fixed-width payload length"),
    ) as usize;

    if HEADER_SIZE + payload_len != bytes.len() {
        return Err(TupleCodecError::InvalidPayloadLength);
    }

    let xmax = xmax_encoded.checked_sub(1);

    let payload = bytes[HEADER_SIZE..].to_vec();

    let mut version = TupleVersion::new(xmin, payload);

    if let Some(xmax) = xmax {
        version.mark_deleted(xmax);
    }

    Ok(version)
}
