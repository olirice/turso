use turso_core::types::ValueIterator;
use turso_core::{decode_array, encode_array, Value};

#[test]
fn encode_array_round_trips_through_a_value_iterator() {
    let values = vec![
        Value::from_i64(1),
        Value::build_text("two".to_string()),
        Value::Null,
    ];
    let encoded = match encode_array(&values) {
        Ok(encoded) => encoded,
        Err(error) => panic!("encoding the array failed: {error}"),
    };
    let Value::Blob(blob) = encoded else {
        panic!("array encoding did not produce a blob");
    };
    let iterator = match ValueIterator::new(&blob) {
        Ok(iterator) => iterator,
        Err(error) => panic!("decoding the array blob failed: {error}"),
    };
    let mut decoded = Vec::new();
    for element in iterator {
        let element = match element {
            Ok(element) => element,
            Err(error) => panic!("reading an array element failed: {error}"),
        };
        match element.to_owned() {
            Ok(value) => decoded.push(value),
            Err(error) => panic!("cloning an array element failed: {error:?}"),
        }
    }
    assert_eq!(decoded, values);
}

#[test]
fn decode_array_reverses_encode_array_and_refuses_a_non_array() {
    let values = vec![Value::from_i64(7), Value::Null];
    let encoded = match encode_array(&values) {
        Ok(encoded) => encoded,
        Err(error) => panic!("encoding the array failed: {error}"),
    };
    match decode_array(&encoded) {
        Ok(decoded) => assert_eq!(decoded, values),
        Err(error) => panic!("decoding the array failed: {error}"),
    }
    assert!(decode_array(&Value::from_i64(1)).is_err());
}
