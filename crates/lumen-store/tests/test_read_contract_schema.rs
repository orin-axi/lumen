//! CRIT-LUMEN-038: every ReadContractEnvelope<T> Lumen serializes must validate against
//! schemas/read-contracts/v1.schema.json, the read contract's source of truth. JSON Schema has
//! no key-ordering construct, so exact key order is verified separately by a golden-file
//! byte-for-byte comparison test below, per the criterion's own text.

use chrono::{TimeZone, Utc};
use compact_str::CompactString;
use lumen_model::{ModelTokenSummary, TokenEconomics};
use lumen_store::{
    FindingReadModel, ReadContractEnvelope, RollupReadModel, SessionDetailReadModel, SessionSummaryReadModel,
    ToolCallReadModel,
};
use std::collections::{BTreeMap, HashMap};

fn schema_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/read-contracts/v1.schema.json")
}

fn validator() -> jsonschema::Validator {
    let schema_text = std::fs::read_to_string(schema_path()).expect("schemas/read-contracts/v1.schema.json must exist");
    let schema_json: serde_json::Value = serde_json::from_str(&schema_text).expect("schema file must be valid JSON");
    jsonschema::validator_for(&schema_json).expect("schema file must itself be a valid JSON Schema")
}

fn sample_economics() -> TokenEconomics {
    let mut per_model = HashMap::new();
    per_model.insert(
        CompactString::from("claude-sonnet-5"),
        ModelTokenSummary {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_tokens: 10,
            cache_read_tokens: 5,
            reasoning_tokens: 2,
            cost_usd: 0.01,
            turns: 3,
            is_fully_priced: true,
        },
    );
    TokenEconomics {
        input_tokens: 100,
        output_tokens: 50,
        cache_creation_tokens: 10,
        cache_read_tokens: 5,
        ephemeral_5m_tokens: 8,
        ephemeral_1h_tokens: 2,
        cache_hit_ratio: 33.3,
        total_cost_usd: 0.01,
        provided_cost_usd: Some(0.01),
        baseline_cost_no_cache_usd: 0.02,
        net_savings_usd: 0.01,
        efficiency_multiplier: 2.0,
        per_model,
        reasoning_output_tokens: 2,
        is_fully_priced: true,
    }
}

fn sample_summary() -> SessionSummaryReadModel {
    SessionSummaryReadModel {
        id: 1,
        provider: "claude-code".to_string(),
        session_id: "sess-1".to_string(),
        model_family: "claude-sonnet-5".to_string(),
        turn_count: 3,
        wall_duration_ms: 1000,
        cache_hit_ratio: 33.3,
        total_cost_usd: 0.01,
        net_savings_usd: 0.01,
        created_at: Utc.with_ymd_and_hms(2026, 8, 22, 0, 0, 0).unwrap(),
    }
}

#[test]
fn test_session_summary_list_envelope_matches_schema() {
    let envelope = ReadContractEnvelope::new(vec![sample_summary()]);
    let instance = serde_json::to_value(&envelope).unwrap();
    let v = validator();
    assert!(v.is_valid(&instance), "SessionSummaryReadModel list envelope failed schema validation: {instance:#}");
}

#[test]
fn test_session_detail_envelope_matches_schema() {
    let mut tool_counts = BTreeMap::new();
    tool_counts.insert(CompactString::from("Read"), 3usize);
    let mut error_counts = BTreeMap::new();
    error_counts.insert(CompactString::from("Read"), 0usize);

    let detail =
        SessionDetailReadModel { summary: sample_summary(), economics: sample_economics(), tool_counts, error_counts };
    let envelope = ReadContractEnvelope::new(detail);
    let instance = serde_json::to_value(&envelope).unwrap();
    let v = validator();
    assert!(v.is_valid(&instance), "SessionDetailReadModel envelope failed schema validation: {instance:#}");
}

#[test]
fn test_finding_envelope_matches_schema() {
    let finding = FindingReadModel {
        id: 1,
        session_id: "sess-1".to_string(),
        rule_id: "RULE-001".to_string(),
        severity: "high".to_string(),
        confidence: 0.9,
        title: "Example finding".to_string(),
        message: "Example message".to_string(),
    };
    let envelope = ReadContractEnvelope::new(finding);
    let instance = serde_json::to_value(&envelope).unwrap();
    let v = validator();
    assert!(v.is_valid(&instance), "FindingReadModel envelope failed schema validation: {instance:#}");
}

#[test]
fn test_tool_call_envelope_matches_schema() {
    let tool_call = ToolCallReadModel {
        id: 1,
        session_id: 1,
        turn_index: 0,
        tool_name: "Read".to_string(),
        call_id: "call-1".to_string(),
        intent_kind: "FileRead".to_string(),
        is_error: false,
        latency_ms: 5,
    };
    let envelope = ReadContractEnvelope::new(tool_call);
    let instance = serde_json::to_value(&envelope).unwrap();
    let v = validator();
    assert!(v.is_valid(&instance), "ToolCallReadModel envelope failed schema validation: {instance:#}");
}

#[test]
fn test_rollup_envelope_matches_schema() {
    let rollup = RollupReadModel {
        id: 1,
        period_start: Utc.with_ymd_and_hms(2026, 8, 22, 0, 0, 0).unwrap(),
        period_type: "daily".to_string(),
        session_count: 5,
        total_cost_usd: 1.0,
        total_savings_usd: 0.5,
        total_duration_ms: 10000,
    };
    let envelope = ReadContractEnvelope::new(rollup);
    let instance = serde_json::to_value(&envelope).unwrap();
    let v = validator();
    assert!(v.is_valid(&instance), "RollupReadModel envelope failed schema validation: {instance:#}");
}

/// An unexpected extra field anywhere must be rejected -- proves additionalProperties:false is
/// actually load-bearing, not just present in the schema text.
#[test]
fn test_envelope_with_unexpected_field_is_rejected() {
    let envelope = ReadContractEnvelope::new(sample_summary());
    let mut instance = serde_json::to_value(&envelope).unwrap();
    instance
        .as_object_mut()
        .unwrap()
        .insert("unexpected_field".to_string(), serde_json::json!("should not be allowed"));

    let v = validator();
    assert!(!v.is_valid(&instance), "an envelope with an unexpected top-level field must fail schema validation");
}

/// A read model missing a required field must be rejected.
#[test]
fn test_envelope_missing_required_field_is_rejected() {
    let envelope = ReadContractEnvelope::new(sample_summary());
    let mut instance = serde_json::to_value(&envelope).unwrap();
    instance.get_mut("data").unwrap().as_object_mut().unwrap().remove("total_cost_usd");

    let v = validator();
    assert!(!v.is_valid(&instance), "an envelope missing a required field must fail schema validation");
}

/// JSON Schema has no key-ordering construct (CRIT-LUMEN-038's own text) -- exact key order is
/// verified here instead, byte-for-byte, against a golden fixture.
#[test]
fn test_session_summary_envelope_key_order_matches_golden_fixture() {
    let envelope = ReadContractEnvelope::new(sample_summary());
    let actual = serde_json::to_string_pretty(&envelope).unwrap();

    let golden_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/session_summary_envelope.json");
    if std::env::var("BLESS_GOLDEN").is_ok() {
        std::fs::write(&golden_path, &actual).unwrap();
    }
    let expected = std::fs::read_to_string(&golden_path).unwrap_or_else(|_| {
        panic!("golden fixture missing at {golden_path:?} -- run once with BLESS_GOLDEN=1 to create it")
    });

    assert_eq!(
        actual.trim_end(),
        expected.trim_end(),
        "envelope's serialized key order must match the golden fixture byte-for-byte"
    );
}
