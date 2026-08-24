use lumen_model::Cost;

#[test]
fn test_cost_serializes_to_usd_priced_shape() {
    let priced = serde_json::to_value(Cost::Priced(12.5)).unwrap();
    assert_eq!(priced, serde_json::json!({"usd": 12.5, "priced": true}));

    let unpriced = serde_json::to_value(Cost::Unpriced).unwrap();
    assert_eq!(unpriced, serde_json::json!({"usd": null, "priced": false}));
}
