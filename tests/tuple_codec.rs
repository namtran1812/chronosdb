use chronosdb::transaction::{TupleCodecError, TupleVersion, decode_tuple, encode_tuple};

#[test]
fn tuple_round_trip_without_xmax() {
    let version = TupleVersion::new(7, b"chronos".to_vec());

    let bytes = encode_tuple(&version);

    let decoded = decode_tuple(&bytes).unwrap();

    assert_eq!(decoded, version);
}

#[test]
fn tuple_round_trip_with_xmax() {
    let mut version = TupleVersion::new(7, b"chronos".to_vec());

    version.mark_deleted(11);

    let decoded = decode_tuple(&encode_tuple(&version)).unwrap();

    assert_eq!(decoded.xmin(), 7);

    assert_eq!(decoded.xmax(), Some(11));

    assert_eq!(decoded.payload(), b"chronos");
}

#[test]
fn truncated_tuple_is_rejected() {
    assert_eq!(decode_tuple(&[0; 10],), Err(TupleCodecError::Truncated));
}

#[test]
fn invalid_payload_length_is_rejected() {
    let version = TupleVersion::new(1, b"value".to_vec());

    let mut bytes = encode_tuple(&version);

    bytes[16..20].copy_from_slice(&999_u32.to_le_bytes());

    assert_eq!(
        decode_tuple(&bytes,),
        Err(TupleCodecError::InvalidPayloadLength)
    );
}
