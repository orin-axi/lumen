use chrono::{DateTime, Utc};
use compact_str::CompactString;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashMap;

use crate::pricing::{PricingTable, TokenRateKind};
use crate::turn::TurnTokenUsage;

/// A cost value that may or may not be genuinely known, pairing the dollar amount with
/// [`TokenEconomics::is_fully_priced`]'s recognition signal at the type level (CRIT-LUMEN-171).
/// The (`f64` total_cost_usd, `bool` is_fully_priced) pair those fields form is still the right
/// representation for computation and storage -- summing costs across turns/sessions and
/// persisting to SQL both need the raw numeric value regardless of pricing status -- but at the
/// point a cost is surfaced to a human (a CLI table, a report), reading `total_cost_usd` alone
/// silently mis-displays an unpriced session as a suspicious-looking `$0.00`. `Cost` makes that
/// mistake a compile error at the display boundary: obtain one via [`TokenEconomics::cost`] and
/// the `Unpriced` case must be handled to get a number out at all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cost {
    /// A dollar amount computed from at least one seeded `PricingTable` row.
    Priced(f64),
    /// No seeded pricing matched the model -- the real cost is unknown, not `$0.00`.
    Unpriced,
}

impl Cost {
    /// Formats this cost for display: `$X.XXXX` when priced, or `unpriced_label` verbatim when
    /// not (e.g. `"unknown (model not in pricing table)"`).
    pub fn format_usd(&self, unpriced_label: &str) -> String {
        match self {
            Cost::Priced(v) => format!("${v:.4}"),
            Cost::Unpriced => unpriced_label.to_string(),
        }
    }
}

impl Serialize for Cost {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Cost", 2)?;
        match self {
            Cost::Priced(v) => {
                state.serialize_field("usd", v)?;
                state.serialize_field("priced", &true)?;
            }
            Cost::Unpriced => {
                state.serialize_field("usd", &Option::<f64>::None)?;
                state.serialize_field("priced", &false)?;
            }
        }
        state.end()
    }
}

/// One turn's token usage plus the pricing context (timestamp, tier) needed to price it
/// independently -- required so a session whose turns straddle a price-change boundary (or a
/// service_tier change) is priced correctly on both sides of the boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnPricingInput {
    pub usage: TurnTokenUsage,
    pub timestamp: DateTime<Utc>,
    pub tier: Option<CompactString>,
}

/// Aggregate session token economics and financial savings metrics.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TokenEconomics {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub ephemeral_5m_tokens: u64,
    pub ephemeral_1h_tokens: u64,
    pub cache_hit_ratio: f32,
    pub total_cost_usd: f64,
    pub provided_cost_usd: Option<f64>,
    pub baseline_cost_no_cache_usd: f64,
    pub net_savings_usd: f64,
    pub efficiency_multiplier: f32,
    pub per_model: HashMap<CompactString, ModelTokenSummary>,
    pub reasoning_output_tokens: u64,
    /// `false` when `model_name` matched no seeded [`PricingTable`] row (see
    /// [`PricingTable::is_recognized`]) -- callers must render cost as an explicit "unknown",
    /// not `$0.00`, in that case. `total_cost_usd`/`net_savings_usd` are still `0.0` for an
    /// unpriced model (every rate lookup returns 0.0), so this flag is the only signal
    /// distinguishing a genuinely free session from an unpriced one.
    pub is_fully_priced: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelTokenSummary {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub reasoning_tokens: u64,
    pub cost_usd: f64,
    pub turns: u64,
    /// See [`TokenEconomics::is_fully_priced`].
    pub is_fully_priced: bool,
}

impl ModelTokenSummary {
    /// See [`TokenEconomics::cost`].
    pub fn cost(&self) -> Cost {
        if self.is_fully_priced {
            Cost::Priced(self.cost_usd)
        } else {
            Cost::Unpriced
        }
    }
}

impl TokenEconomics {
    /// Computes full economic summary from one [`TurnPricingInput`] per turn. Each turn is
    /// priced independently via `pricing.rate_for` at that turn's own timestamp/tier, and the
    /// per-turn costs are summed into `total_cost_usd` -- required so a session whose turns
    /// straddle a price-change boundary is priced correctly on both sides. Token counters
    /// remain simple sums across turns, unaffected by the per-turn pricing. An empty `turns`
    /// slice returns a zeroed `TokenEconomics`, not an error.
    pub fn calculate(
        turns: &[TurnPricingInput],
        model_name: &str,
        pricing: &PricingTable,
        provided_cost_usd: Option<f64>,
    ) -> Self {
        let is_fully_priced = pricing.is_recognized(model_name);
        // Narrows the (now ~1,500+ row, post-CRIT-LUMEN-170) pricing table down to this model's
        // rows once, outside the loop below, instead of re-scanning the full table on every one
        // of the 6 rate_for calls per turn.
        let rows = pricing.rows_for_model(model_name);

        let mut input_tokens = 0u64;
        let mut output_tokens = 0u64;
        let mut cache_creation_tokens = 0u64;
        let mut cache_creation_1h_tokens = 0u64;
        let mut cache_read_tokens = 0u64;
        let mut reasoning_tokens_total = 0u64;
        let mut total_cost_usd = 0.0f64;
        let mut baseline_cost_no_cache_usd = 0.0f64;

        for turn in turns {
            let usage = &turn.usage;
            let tier = turn.tier.as_deref();

            let input_rate = PricingTable::rate_for_rows(&rows, tier, TokenRateKind::Input, turn.timestamp);
            let cache_write_5m_rate =
                PricingTable::rate_for_rows(&rows, tier, TokenRateKind::CacheWrite, turn.timestamp);
            let cache_write_1h_rate =
                PricingTable::rate_for_rows(&rows, tier, TokenRateKind::CacheWrite1h, turn.timestamp);
            let cache_read_rate = PricingTable::rate_for_rows(&rows, tier, TokenRateKind::CacheRead, turn.timestamp);
            let output_rate = PricingTable::rate_for_rows(&rows, tier, TokenRateKind::Output, turn.timestamp);
            let reasoning_rate = PricingTable::rate_for_rows(&rows, tier, TokenRateKind::Reasoning, turn.timestamp);

            let cache_write_5m_tokens = usage.cache_creation_tokens.saturating_sub(usage.cache_creation_1h_tokens);

            let turn_cost = (usage.input_tokens as f64 / 1_000_000.0) * input_rate
                + (cache_write_5m_tokens as f64 / 1_000_000.0) * cache_write_5m_rate
                + (usage.cache_creation_1h_tokens as f64 / 1_000_000.0) * cache_write_1h_rate
                + (usage.cache_read_tokens as f64 / 1_000_000.0) * cache_read_rate
                + (usage.output_tokens as f64 / 1_000_000.0) * output_rate
                + (usage.reasoning_tokens as f64 / 1_000_000.0) * reasoning_rate;

            // Reasoning tokens are priced at the same rate in both the actual and no-cache
            // baseline cost -- caching doesn't apply to reasoning tokens, so omitting this term
            // from one side but not the other would skew net_savings_usd/efficiency_multiplier.
            let turn_baseline_cost = (usage.prompt_tokens() as f64 / 1_000_000.0) * input_rate
                + (usage.output_tokens as f64 / 1_000_000.0) * output_rate
                + (usage.reasoning_tokens as f64 / 1_000_000.0) * reasoning_rate;

            input_tokens += usage.input_tokens;
            output_tokens += usage.output_tokens;
            cache_creation_tokens += usage.cache_creation_tokens;
            cache_creation_1h_tokens += usage.cache_creation_1h_tokens;
            cache_read_tokens += usage.cache_read_tokens;
            reasoning_tokens_total += usage.reasoning_tokens;
            total_cost_usd += turn_cost;
            baseline_cost_no_cache_usd += turn_baseline_cost;
        }

        let prompt_total = input_tokens + cache_creation_tokens + cache_read_tokens;
        let cache_hit_ratio =
            if prompt_total > 0 { (cache_read_tokens as f32 / prompt_total as f32) * 100.0 } else { 0.0 };

        let net_savings_usd = (baseline_cost_no_cache_usd - total_cost_usd).max(0.0);

        let efficiency_multiplier =
            if total_cost_usd > 0.0 { (baseline_cost_no_cache_usd / total_cost_usd) as f32 } else { 1.0 };

        let mut per_model = HashMap::new();
        if !turns.is_empty() {
            per_model.insert(
                CompactString::new(model_name),
                ModelTokenSummary {
                    input_tokens,
                    output_tokens,
                    cache_creation_tokens,
                    cache_read_tokens,
                    reasoning_tokens: reasoning_tokens_total,
                    cost_usd: total_cost_usd,
                    turns: turns.len() as u64,
                    is_fully_priced,
                },
            );
        }

        Self {
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            ephemeral_5m_tokens: cache_creation_tokens.saturating_sub(cache_creation_1h_tokens),
            ephemeral_1h_tokens: cache_creation_1h_tokens,
            cache_hit_ratio,
            total_cost_usd,
            provided_cost_usd,
            baseline_cost_no_cache_usd,
            net_savings_usd,
            efficiency_multiplier,
            per_model,
            reasoning_output_tokens: reasoning_tokens_total,
            is_fully_priced,
        }
    }

    /// Surfaces `total_cost_usd`/`is_fully_priced` as a single [`Cost`] value -- the
    /// CRIT-LUMEN-171 display-boundary accessor. Prefer this over reading `total_cost_usd`
    /// directly whenever the value is headed somewhere a human will read it.
    pub fn cost(&self) -> Cost {
        if self.is_fully_priced {
            Cost::Priced(self.total_cost_usd)
        } else {
            Cost::Unpriced
        }
    }
}
