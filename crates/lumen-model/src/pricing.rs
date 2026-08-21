use chrono::{DateTime, TimeZone, Utc};
use compact_str::CompactString;

use crate::turn::TurnTokenUsage;

/// Kind of token rate a [`PricingRate`] row applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenRateKind {
    Input,
    CacheWrite,
    CacheRead,
    Output,
}

/// A single versioned pricing row: the rate for one (model, tier, kind) combination,
/// valid over an [effective_from, effective_until) window.
#[derive(Debug, Clone, PartialEq)]
pub struct PricingRate {
    pub model: CompactString,
    pub tier: Option<CompactString>,
    pub kind: TokenRateKind,
    pub rate_per_m: f64,
    pub effective_from: DateTime<Utc>,
    pub effective_until: Option<DateTime<Utc>>,
}

/// Versioned exchange-rate-style pricing lookup table, replacing the hardcoded
/// [`ModelPricing`] match statement with dated rows.
#[derive(Debug, Clone, PartialEq)]
pub struct PricingTable {
    pub rates: Vec<PricingRate>,
}

/// Shared baseline epoch for all seeded rows' `effective_from`.
fn seed_epoch() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
}

impl PricingTable {
    /// Seeds the table with the canonical rate rows for the models this pass covers.
    pub fn seed() -> Self {
        let epoch = seed_epoch();
        let mut rates = Vec::new();

        let mut push = |model: &str, kind: TokenRateKind, rate_per_m: f64| {
            rates.push(PricingRate {
                model: CompactString::from(model),
                tier: None,
                kind,
                rate_per_m,
                effective_from: epoch,
                effective_until: None,
            });
        };

        push("claude-3-5-sonnet", TokenRateKind::Input, 3.00);
        push("claude-3-5-sonnet", TokenRateKind::CacheWrite, 3.75);
        push("claude-3-5-sonnet", TokenRateKind::CacheRead, 0.30);
        push("claude-3-5-sonnet", TokenRateKind::Output, 15.00);

        push("claude-3-5-haiku", TokenRateKind::Input, 0.80);
        push("claude-3-5-haiku", TokenRateKind::CacheWrite, 1.00);
        push("claude-3-5-haiku", TokenRateKind::CacheRead, 0.08);
        push("claude-3-5-haiku", TokenRateKind::Output, 4.00);

        push("qwen-2.5-coder", TokenRateKind::Input, 0.20);
        push("qwen-2.5-coder", TokenRateKind::CacheRead, 0.05);
        push("qwen-2.5-coder", TokenRateKind::Output, 0.60);

        // CRIT-LUMEN-099: Claude Opus
        push("claude-opus", TokenRateKind::Input, 15.00);
        push("claude-opus", TokenRateKind::CacheWrite, 18.75);
        push("claude-opus", TokenRateKind::CacheRead, 1.50);
        push("claude-opus", TokenRateKind::Output, 75.00);

        // CRIT-LUMEN-100: GPT-4o
        push("gpt-4o", TokenRateKind::Input, 2.50);
        push("gpt-4o", TokenRateKind::CacheWrite, 2.50);
        push("gpt-4o", TokenRateKind::CacheRead, 1.25);
        push("gpt-4o", TokenRateKind::Output, 10.00);

        // CRIT-LUMEN-101: DeepSeek R1
        push("deepseek-r1", TokenRateKind::Input, 0.55);
        push("deepseek-r1", TokenRateKind::CacheWrite, 0.55);
        push("deepseek-r1", TokenRateKind::CacheRead, 0.14);
        push("deepseek-r1", TokenRateKind::Output, 2.19);

        // CRIT-LUMEN-102: Kimi K1.5
        push("kimi-k1.5", TokenRateKind::Input, 0.50);
        push("kimi-k1.5", TokenRateKind::CacheWrite, 0.50);
        push("kimi-k1.5", TokenRateKind::CacheRead, 0.10);
        push("kimi-k1.5", TokenRateKind::Output, 2.00);

        // CRIT-LUMEN-103: GLM-4-Plus
        push("glm-4-plus", TokenRateKind::Input, 1.40);
        push("glm-4-plus", TokenRateKind::CacheWrite, 1.40);
        push("glm-4-plus", TokenRateKind::CacheRead, 0.20);
        push("glm-4-plus", TokenRateKind::Output, 1.40);

        // CRIT-LUMEN-104: Gemini 2.0 Flash
        push("gemini-2.0-flash", TokenRateKind::Input, 0.10);
        push("gemini-2.0-flash", TokenRateKind::CacheWrite, 0.10);
        push("gemini-2.0-flash", TokenRateKind::CacheRead, 0.025);
        push("gemini-2.0-flash", TokenRateKind::Output, 0.40);

        // CRIT-LUMEN-105: Gemini 2.0 Pro
        push("gemini-2.0-pro", TokenRateKind::Input, 1.25);
        push("gemini-2.0-pro", TokenRateKind::CacheWrite, 1.25);
        push("gemini-2.0-pro", TokenRateKind::CacheRead, 0.30);
        push("gemini-2.0-pro", TokenRateKind::Output, 5.00);

        Self { rates }
    }

    /// Looks up the rate for (model, tier, kind) whose effective window contains `as_of`.
    /// Falls back to `claude-3-5-sonnet`'s rate for the requested kind when the model
    /// string matches no row at all (CRIT-LUMEN-008). When the model IS recognized (matches
    /// at least one row) but has no row for this specific (tier, kind, as_of), returns 0.0
    /// directly rather than substituting another model's rate (CRIT-LUMEN-161).
    pub fn rate_for(&self, model: &str, tier: Option<&str>, kind: TokenRateKind, as_of: DateTime<Utc>) -> f64 {
        let model_recognized = self.rates.iter().any(|r| r.model == model);

        let lookup_model = if model_recognized { model } else { "claude-3-5-sonnet" };

        self.rates
            .iter()
            .filter(|r| {
                r.model == lookup_model
                    && r.kind == kind
                    && r.tier.as_deref() == tier
                    && r.effective_from <= as_of
                    && r.effective_until.is_none_or(|until| as_of < until)
            })
            .max_by_key(|r| r.effective_from)
            .map(|r| r.rate_per_m)
            .unwrap_or(0.0)
    }
}

/// Model pricing matrix defining exact rates per 1,000,000 tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPricing {
    pub input_base_per_m: f64,
    pub cache_write_per_m: f64,
    pub cache_read_per_m: f64,
    pub output_per_m: f64,
}

impl ModelPricing {
    pub const CLAUDE_3_5_SONNET: Self = Self {
        input_base_per_m: 3.00,
        cache_write_per_m: 3.75, // 1.25x
        cache_read_per_m: 0.30,  // 0.10x (90% savings)
        output_per_m: 15.00,
    };

    pub const CLAUDE_3_5_HAIKU: Self =
        Self { input_base_per_m: 0.80, cache_write_per_m: 1.00, cache_read_per_m: 0.08, output_per_m: 4.00 };

    pub const CLAUDE_OPUS: Self =
        Self { input_base_per_m: 15.00, cache_write_per_m: 18.75, cache_read_per_m: 1.50, output_per_m: 75.00 };

    pub const GPT_4O: Self =
        Self { input_base_per_m: 2.50, cache_write_per_m: 2.50, cache_read_per_m: 1.25, output_per_m: 10.00 };

    pub const DEEPSEEK_R1: Self =
        Self { input_base_per_m: 0.55, cache_write_per_m: 0.55, cache_read_per_m: 0.14, output_per_m: 2.19 };

    pub const QWEN_2_5_CODER: Self =
        Self { input_base_per_m: 0.20, cache_write_per_m: 0.20, cache_read_per_m: 0.05, output_per_m: 0.60 };

    pub const KIMI_K1_5: Self =
        Self { input_base_per_m: 0.50, cache_write_per_m: 0.50, cache_read_per_m: 0.10, output_per_m: 2.00 };

    pub const GLM_4_PLUS: Self =
        Self { input_base_per_m: 1.40, cache_write_per_m: 1.40, cache_read_per_m: 0.20, output_per_m: 1.40 };

    pub const GEMINI_2_0_FLASH: Self =
        Self { input_base_per_m: 0.10, cache_write_per_m: 0.10, cache_read_per_m: 0.025, output_per_m: 0.40 };

    pub const GEMINI_2_0_PRO: Self =
        Self { input_base_per_m: 1.25, cache_write_per_m: 1.25, cache_read_per_m: 0.30, output_per_m: 5.00 };

    /// Resolves pricing from model identifier string.
    pub fn from_model_name(name: &str) -> Self {
        let lower = name.to_lowercase();
        if lower.contains("haiku") {
            Self::CLAUDE_3_5_HAIKU
        } else if lower.contains("opus") {
            Self::CLAUDE_OPUS
        } else if lower.contains("gpt-4o") {
            Self::GPT_4O
        } else if lower.contains("deepseek") {
            Self::DEEPSEEK_R1
        } else if lower.contains("qwen") {
            Self::QWEN_2_5_CODER
        } else if lower.contains("kimi") || lower.contains("moonshot") {
            Self::KIMI_K1_5
        } else if lower.contains("glm") || lower.contains("zhipu") {
            Self::GLM_4_PLUS
        } else if lower.contains("gemini-2.0-flash") || lower.contains("flash") {
            Self::GEMINI_2_0_FLASH
        } else if lower.contains("gemini-2.0-pro") || lower.contains("gemini") {
            Self::GEMINI_2_0_PRO
        } else {
            // Default to Claude 3.5 Sonnet
            Self::CLAUDE_3_5_SONNET
        }
    }

    /// Computes exact USD cost for a token usage turn.
    pub fn compute_cost(&self, usage: &TurnTokenUsage) -> f64 {
        let uncached_cost = (usage.input_tokens as f64 / 1_000_000.0) * self.input_base_per_m;
        let write_cost = (usage.cache_creation_tokens as f64 / 1_000_000.0) * self.cache_write_per_m;
        let read_cost = (usage.cache_read_tokens as f64 / 1_000_000.0) * self.cache_read_per_m;
        let output_cost = (usage.output_tokens as f64 / 1_000_000.0) * self.output_per_m;

        uncached_cost + write_cost + read_cost + output_cost
    }

    /// Computes hypothetical baseline cost if no prompt caching was utilized.
    pub fn compute_baseline_cost(&self, usage: &TurnTokenUsage) -> f64 {
        let total_prompt = usage.prompt_tokens() as f64;
        let prompt_cost = (total_prompt / 1_000_000.0) * self.input_base_per_m;
        let output_cost = (usage.output_tokens as f64 / 1_000_000.0) * self.output_per_m;

        prompt_cost + output_cost
    }
}
