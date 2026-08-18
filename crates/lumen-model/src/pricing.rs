use crate::turn::TurnTokenUsage;

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
