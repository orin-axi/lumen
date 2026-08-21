use lumen_model::*;

#[test]
fn test_parse_failure_record_roundtrip() {
    let record = ParseFailureRecord {
        session_id: "sess-1".into(),
        line_number: 4,
        byte_offset: 128,
        error: "unexpected end of input".into(),
    };
    let json = serde_json::to_string(&record).unwrap();
    let back: ParseFailureRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(record, back);
}
