use chrono::{DateTime, Utc};
use compact_str::CompactString;
use serde_json::Value;

use crate::pricing::{PricingRate, TokenRateKind};

/// Vendored snapshot of LiteLLM's community-maintained `model_prices_and_context_window.json`
/// (github.com/BerriAI/litellm), refreshed via `just update-pricing` (CRIT-LUMEN-170). Never
/// fetched live: Lumen prices historical sessions and needs point-in-time-stable rates, so this
/// is a periodic, explicit re-vendor, not a per-request fetch.
const VENDORED_JSON: &str = include_str!("../data/litellm_model_prices.json");

/// `litellm_provider` values treated as a model's direct, first-party API -- never a resale or
/// regional gateway. Covers exactly the providers Lumen's four known adapters actually draw from
/// (Claude Code: anthropic; Codex: openai; AGY (Gemini-lineage) and OpenCode: gemini /
/// vertex_ai-language-models). A gateway/reseller entry for the SAME underlying model (LiteLLM
/// separately lists "bedrock_converse", "azure_ai", "vertex_ai-anthropic_models", etc. rows per
/// model, often at region-marked-up rates) is deliberately excluded: none of Lumen's adapters
/// have ever observed a raw `model` string in that shape, and including every gateway listing
/// would multiply the seeded row count ~20x for no real coverage gain. This is a mechanical
/// filter applied uniformly to whatever LiteLLM publishes under these providers -- not
/// per-model hand-picking -- so new models under an already-allowed provider need no code
/// change here. Revisit if a future adapter targets a genuinely different first-party provider
/// (e.g. a direct DeepSeek, Moonshot, or Mistral API integration).
const DIRECT_PROVIDERS: &[&str] = &["anthropic", "openai", "gemini", "vertex_ai-language-models"];

/// Reads `field` off a LiteLLM model entry as dollars-per-token and converts it to Lumen's
/// dollars-per-million-tokens `rate_per_m` unit. Returns `None` when the field is absent or not
/// a number -- LiteLLM omits a field entirely when a provider genuinely doesn't publish a rate
/// for that token kind, and that absence must propagate as "no row" (giving `rate_for` its
/// existing 0.0-not-a-substitution behavior, CRIT-LUMEN-161), not an invented value.
fn field_rate_per_m(entry: &serde_json::Map<String, Value>, field: &str) -> Option<f64> {
    entry.get(field).and_then(Value::as_f64).map(|per_token| per_token * 1_000_000.0)
}

/// Parses the vendored LiteLLM snapshot into `PricingRate` rows, all sharing `epoch` as
/// `effective_from` -- the same shared epoch every other seeded row uses (see
/// `PricingTable::seed`'s doc comment for why this table has no real historical
/// price-change versioning yet; vendoring a fresher snapshot does not change that).
///
/// Only `"mode": "chat"` entries under a [`DIRECT_PROVIDERS`] provider are considered. For each
/// such entry, one row is pushed per pricing field actually present (`input_cost_per_token` ->
/// [`TokenRateKind::Input`], `output_cost_per_token` -> [`TokenRateKind::Output`],
/// `cache_read_input_token_cost` -> [`TokenRateKind::CacheRead`],
/// `cache_creation_input_token_cost` -> [`TokenRateKind::CacheWrite`],
/// `cache_creation_input_token_cost_above_1hr` -> [`TokenRateKind::CacheWrite1h`]) -- a field
/// LiteLLM omits produces no row, never an invented one. `output_cost_per_reasoning_token`
/// maps to [`TokenRateKind::Reasoning`] when present; when absent but an output rate exists, a
/// Reasoning row is pushed equal to the Output rate, preserving the judgment call this table
/// already made for its original hand-seeded rows: every provider observed bills reasoning
/// tokens at the same rate as completion/output tokens, and no seeded provider publishes a
/// distinct reasoning rate except where LiteLLM states one explicitly.
pub(crate) fn load_vendored_rates(epoch: DateTime<Utc>) -> Vec<PricingRate> {
    let root: Value = serde_json::from_str(VENDORED_JSON)
        .expect("vendored litellm_model_prices.json must be valid JSON (checked in, not user input)");

    let Some(entries) = root.as_object() else {
        return Vec::new();
    };

    let mut rates = Vec::new();

    for (model_key, entry) in entries {
        let Some(entry) = entry.as_object() else { continue };

        if entry.get("mode").and_then(Value::as_str) != Some("chat") {
            continue;
        }
        let provider = entry.get("litellm_provider").and_then(Value::as_str).unwrap_or("");
        if !DIRECT_PROVIDERS.contains(&provider) {
            continue;
        }

        let mut push = |kind: TokenRateKind, rate_per_m: f64| {
            rates.push(PricingRate {
                model: CompactString::from(model_key.as_str()),
                tier: None,
                kind,
                rate_per_m,
                effective_from: epoch,
                effective_until: None,
            });
        };

        if let Some(r) = field_rate_per_m(entry, "input_cost_per_token") {
            push(TokenRateKind::Input, r);
        }
        let output_rate = field_rate_per_m(entry, "output_cost_per_token");
        if let Some(r) = output_rate {
            push(TokenRateKind::Output, r);
        }
        if let Some(r) = field_rate_per_m(entry, "cache_read_input_token_cost") {
            push(TokenRateKind::CacheRead, r);
        }
        if let Some(r) = field_rate_per_m(entry, "cache_creation_input_token_cost") {
            push(TokenRateKind::CacheWrite, r);
        }
        if let Some(r) = field_rate_per_m(entry, "cache_creation_input_token_cost_above_1hr") {
            push(TokenRateKind::CacheWrite1h, r);
        }

        match field_rate_per_m(entry, "output_cost_per_reasoning_token") {
            Some(r) => push(TokenRateKind::Reasoning, r),
            None => {
                if let Some(r) = output_rate {
                    push(TokenRateKind::Reasoning, r);
                }
            }
        }
    }

    rates
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn epoch() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    }

    /// A real, currently-active first-party model must be loaded with real, nonzero rates for
    /// every field the vendored source actually publishes for it.
    #[test]
    fn test_load_vendored_rates_includes_direct_provider_chat_model() {
        let rates = load_vendored_rates(epoch());
        let claude_opus_5: Vec<_> = rates.iter().filter(|r| r.model == "claude-opus-5").collect();

        assert!(!claude_opus_5.is_empty(), "claude-opus-5 must be loaded from the vendored snapshot");
        assert!(claude_opus_5.iter().any(|r| r.kind == TokenRateKind::Input && (r.rate_per_m - 5.00).abs() < 1e-9));
        assert!(claude_opus_5.iter().any(|r| r.kind == TokenRateKind::Output && (r.rate_per_m - 25.00).abs() < 1e-9));
        assert!(claude_opus_5.iter().all(|r| r.effective_from == epoch() && r.effective_until.is_none()));
    }

    /// CRIT-LUMEN-170's DIRECT_PROVIDERS filter must exclude gateway/regional resale listings
    /// (LiteLLM lists the same underlying model many times over under bedrock/azure/regional
    /// prefixes, often at marked-up or region-specific rates) -- only the model's own direct,
    /// first-party listing is loaded.
    #[test]
    fn test_load_vendored_rates_excludes_gateway_and_regional_listings() {
        let rates = load_vendored_rates(epoch());

        for gateway_key in ["au.anthropic.claude-opus-5", "anthropic.claude-opus-5", "azure_ai/claude-opus-4-5"] {
            assert!(
                !rates.iter().any(|r| r.model == gateway_key),
                "{gateway_key} is a gateway/regional listing and must not be loaded"
            );
        }
    }

    /// Only `"mode": "chat"` entries are loaded -- embeddings, image-generation, audio, and
    /// other non-chat modes are outside what Lumen's adapters ever report as a session's model.
    #[test]
    fn test_load_vendored_rates_excludes_non_chat_modes() {
        let rates = load_vendored_rates(epoch());
        assert!(
            !rates.iter().any(|r| r.model == "text-embedding-3-small"),
            "an embeddings-mode model must not be loaded as a chat pricing row"
        );
    }

    /// A field LiteLLM omits for a real model must produce no row at all -- never an invented
    /// rate (e.g. gpt-4o publishes no cache-write rate; unlike the prior hand-typed
    /// same-as-input approximation, this must NOT synthesize a CacheWrite row for it).
    #[test]
    fn test_load_vendored_rates_omits_rows_for_absent_fields_rather_than_inventing_them() {
        let rates = load_vendored_rates(epoch());
        assert!(
            !rates.iter().any(|r| r.model == "gpt-4o" && r.kind == TokenRateKind::CacheWrite),
            "gpt-4o has no published cache-write rate and must get no CacheWrite row at all"
        );
        assert!(rates.iter().any(|r| r.model == "gpt-4o" && r.kind == TokenRateKind::Input));
    }

    /// When a model publishes no distinct reasoning-token rate, a Reasoning row is synthesized
    /// equal to the Output rate (the existing judgment call this table already made for its
    /// hand-seeded rows, generalized to vendored data); when one IS published, that real rate is
    /// used instead of the synthesized fallback.
    #[test]
    fn test_load_vendored_rates_reasoning_fallback_and_explicit_rate() {
        let rates = load_vendored_rates(epoch());

        let gpt4o_output = rates.iter().find(|r| r.model == "gpt-4o" && r.kind == TokenRateKind::Output).unwrap();
        let gpt4o_reasoning = rates.iter().find(|r| r.model == "gpt-4o" && r.kind == TokenRateKind::Reasoning).unwrap();
        assert!((gpt4o_output.rate_per_m - gpt4o_reasoning.rate_per_m).abs() < 1e-9);

        let flash_reasoning = rates
            .iter()
            .find(|r| r.model == "gemini-3.7-flash" && r.kind == TokenRateKind::Reasoning)
            .expect("gemini-3.7-flash publishes an explicit reasoning rate");
        assert!((flash_reasoning.rate_per_m - 3.75).abs() < 1e-9);
    }
}
