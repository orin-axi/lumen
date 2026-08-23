use chrono::{DateTime, TimeZone, Utc};
use compact_str::CompactString;

/// Kind of token rate a [`PricingRate`] row applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenRateKind {
    Input,
    /// Default/short-lived cache write (Anthropic's 5-minute ephemeral tier; the only cache
    /// write tier for providers that don't publish a separate long-lived rate).
    CacheWrite,
    /// Long-lived cache write (Anthropic's 1-hour ephemeral tier). Only seeded for providers
    /// that publish a distinct rate for it -- see CacheWrite's doc comment.
    CacheWrite1h,
    CacheRead,
    Output,
    Reasoning,
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

/// Versioned exchange-rate-style pricing lookup table, replacing the removed hardcoded
/// ModelPricing match statement with dated rows.
#[derive(Debug, Clone, PartialEq)]
pub struct PricingTable {
    pub rates: Vec<PricingRate>,
}

/// Shared baseline epoch for all seeded rows' `effective_from`.
fn seed_epoch() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
}

/// Shared, lazily-initialized seeded pricing table -- built once on first access and reused by
/// every caller, instead of each adapter rebuilding a fresh ~40-row `PricingTable` on every
/// single file it parses. `TokenEconomics::calculate` takes `&PricingTable` specifically so
/// callers can share one instance like this; `PricingTable::seed()` itself remains a real,
/// public, useful constructor (used by tests that build their own tables, and by this static
/// itself) -- this is purely about hot-path call sites reusing it instead of rebuilding it.
pub static SEEDED: std::sync::LazyLock<PricingTable> = std::sync::LazyLock::new(PricingTable::seed);

impl PricingTable {
    /// Seeds the table with two layers of rate rows, both sharing the same `effective_from`
    /// epoch (this table does not yet do real historical price-change versioning -- every row,
    /// old and new, is treated as effective from the same fixed point onward; see
    /// [`seed_epoch`]):
    ///
    /// 1. A small hand-typed *legacy* layer (below) for models that were real, actively-used,
    ///    and individually verified against official pricing pages at some point, but have since
    ///    rolled off LiteLLM's actively-maintained current snapshot entirely (superseded by
    ///    newer model generations in the vendored source -- CRIT-LUMEN-170's research confirmed
    ///    this directly: `claude-3-5-sonnet`, `claude-3-5-haiku`, `qwen-2.5-coder`, `claude-opus`
    ///    (the v3 family), `deepseek-r1`, `kimi-k1.5`, `glm-4-plus`, and `gemini-2.0-pro` are
    ///    genuinely absent from the current upstream file under any close variant of these
    ///    names). Lumen prices historical sessions, so a model dropping out of LiteLLM's
    ///    *current* snapshot must not make real historical sessions using it go unpriced --
    ///    these rows are deliberately retained by hand rather than deleted.
    /// 2. The bulk of the table, loaded from [`crate::pricing_source::load_vendored_rates`]: a
    ///    vendored copy of LiteLLM's community-maintained `model_prices_and_context_window.json`
    ///    (CRIT-LUMEN-170), refreshed via `just update-pricing`, not hand-typed. This replaces
    ///    what used to be a second hand-typed block for every "current-generation" model
    ///    (`claude-fable-5`, `claude-opus-5`, `claude-sonnet-5`, `claude-haiku-4-5`,
    ///    `gpt-5.6-terra`, `gemini-3.7-flash`, plus the still-current `gpt-4o` and
    ///    `gemini-2.0-flash`) -- cross-checked against real Opus 5 rates on 2026-08-22 and found
    ///    to match Lumen's prior hand-typed values exactly for every field LiteLLM actually
    ///    publishes. Two corrections fell out of that cross-check and are intentional, not
    ///    regressions: `gpt-5.6-terra`'s cache-write rate is real ($2.50/M, not the same as its
    ///    $2.00/M input rate as previously assumed), and `gpt-4o`/`gemini-2.0-flash` publish no
    ///    cache-write rate at all (so `rate_for` now correctly returns 0.0 for that kind per
    ///    CRIT-LUMEN-161, instead of the previous same-as-input approximation).
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

        // CRIT-LUMEN-099: Claude Opus (v3 family)
        push("claude-opus", TokenRateKind::Input, 15.00);
        push("claude-opus", TokenRateKind::CacheWrite, 18.75);
        push("claude-opus", TokenRateKind::CacheRead, 1.50);
        push("claude-opus", TokenRateKind::Output, 75.00);

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

        // CRIT-LUMEN-105: Gemini 2.0 Pro
        push("gemini-2.0-pro", TokenRateKind::Input, 1.25);
        push("gemini-2.0-pro", TokenRateKind::CacheWrite, 1.25);
        push("gemini-2.0-pro", TokenRateKind::CacheRead, 0.30);
        push("gemini-2.0-pro", TokenRateKind::Output, 5.00);

        // Reasoning rows for the legacy layer only: the vendored layer supplies its own (see
        // pricing_source::load_vendored_rates). Adversarial finding (high severity, original to
        // this table): reasoning tokens were tracked (TurnTokenUsage/
        // TokenEconomics::reasoning_output_tokens) but had no rate row at all, so no adapter
        // could ever price them. Every provider observed bills reasoning tokens as part of
        // completion/output tokens, at the SAME published output rate -- "same as Output" is
        // the closest defensible default absent a provider-published distinct reasoning rate.
        // Each value below is copied verbatim from that same model's Output row pushed above --
        // deliberately duplicated, not looked up, so this loop can run without re-borrowing
        // `rates` while `push` still holds it mutably.
        for (model, output_rate) in [
            ("claude-3-5-sonnet", 15.00),
            ("claude-3-5-haiku", 4.00),
            ("qwen-2.5-coder", 0.60),
            ("claude-opus", 75.00),
            ("deepseek-r1", 2.19),
            ("kimi-k1.5", 2.00),
            ("glm-4-plus", 1.40),
            ("gemini-2.0-pro", 5.00),
        ] {
            push(model, TokenRateKind::Reasoning, output_rate);
        }

        rates.extend(crate::pricing_source::load_vendored_rates(epoch));

        Self { rates }
    }

    /// Normalizes a raw, provider-versioned model string (e.g. "claude-3-5-haiku-20241022",
    /// "gpt-4o-2024-08-06", "gemini-2.0-flash-001") down to one of this table's seeded
    /// canonical keys, by trying progressively shorter '-'-delimited prefixes of `model`
    /// against the set of model names actually present in `self.rates`, longest prefix first.
    /// Returns `None` when no prefix matches any seeded key -- callers must fall back to the
    /// CRIT-LUMEN-008 unrecognized-model behavior in that case.
    ///
    /// A small explicit alias table was considered instead, but seeded keys are themselves
    /// hyphen-delimited words (e.g. "claude-3-5-haiku") and every real-world date/version
    /// suffix observed (YYYYMMDD, YYYY-MM-DD, "-001", "-exp", "-32b-instruct") is just extra
    /// hyphen-delimited segments appended after the canonical name. Prefix-shortening solves
    /// all of them with one algorithm and needs no maintenance as new dated releases appear,
    /// whereas an alias table would need a new entry per new version string.
    fn normalize_model_key<'a>(&'a self, model: &'a str) -> Option<&'a str> {
        let parts: Vec<&str> = model.split('-').collect();
        for end in (1..=parts.len()).rev() {
            // Only accept this (shorter) candidate if every segment it would strip from the
            // tail is a safe version/date/scale marker -- never a segment that could denote a
            // genuinely different, differently-priced model (e.g. "mini", "lite", "nano").
            if !parts[end..].iter().all(|seg| Self::is_safe_strip_segment(seg)) {
                continue;
            }
            let candidate = &model[..Self::prefix_byte_len(&parts, end)];
            if self.rates.iter().any(|r| r.model == candidate) {
                return Some(candidate);
            }
        }
        None
    }

    /// Whether a single '-'-delimited segment stripped from the tail of a raw model string is
    /// safe to discard when normalizing -- i.e. it can only ever denote a version, release
    /// date, or parameter-scale marker, never a genuinely different priced model name.
    ///
    /// Deliberately an ALLOWLIST of safe-to-strip patterns, not a denylist of known-bad words:
    /// a denylist can never be complete (new differently-priced model tiers are named
    /// constantly -- "mini", "nano", "flash-lite", ...), whereas the set of ways providers
    /// encode version/date/scale is small and closed.
    ///
    /// 1. Numeric-only segments ("20241022", "001", "0528", "4", "1") -- a bare number is never
    ///    a model-tier name.
    /// 2. Parameter-scale segments matching `^\d+[A-Za-z]$` ("32b", "70b") -- digits followed by
    ///    exactly one trailing letter denote model SIZE within the same family/price tier, not
    ///    a different-tier product name. Deliberately the strict single-trailing-letter form,
    ///    not a looser pattern that would also admit multi-token scale strings like "8x7b" --
    ///    no seeded key requires stripping that shape, so the stricter (safer) form is kept.
    /// 3. A small explicit allowlist of known non-differentiating tuning-suffix words.
    fn is_safe_strip_segment(segment: &str) -> bool {
        if !segment.is_empty() && segment.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }

        if let Some(last) = segment.chars().next_back() {
            if last.is_ascii_alphabetic() {
                let digits = &segment[..segment.len() - last.len_utf8()];
                if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                    return true;
                }
            }
        }

        // "fast"/"high" cover real observed request-time speed/effort variant suffixes
        // (gpt-5.6-terra-fast, gemini-3.7-flash-high) that carry no separately published price
        // for their provider. This is a deliberate, narrower judgment call than the rest of this
        // allowlist: Anthropic's "fast mode" IS a genuinely different, separately published
        // 2x-priced product (confirmed on platform.claude.com/docs/en/about-claude/pricing) --
        // but it is applied via a request parameter (`speed: "fast"`), never observed embedded
        // in a real `message.model` string this session. If a provider ever starts encoding
        // "-fast"/"-high" as part of the model name string for a genuinely different-priced
        // tier, this allowlist entry would silently mis-price it -- revisit if that's observed.
        matches!(segment, "exp" | "latest" | "preview" | "instruct" | "chat" | "fast" | "high")
    }

    /// Byte length of the '-'-joined prefix consisting of the first `count` elements of
    /// `parts` (which were produced by splitting the original string on '-').
    fn prefix_byte_len(parts: &[&str], count: usize) -> usize {
        let joined_len: usize = parts[..count].iter().map(|p| p.len()).sum();
        // account for the (count - 1) '-' separators between the included parts
        joined_len + count.saturating_sub(1)
    }

    /// Whether `model` -- after normalizing away any provider-versioning suffix -- matches at
    /// least one seeded row. Callers use this to distinguish "priced at 0.0 because this model
    /// genuinely has no cost for this token kind" from "priced at 0.0 because this model has no
    /// seeded pricing at all" (an unrecognized model), the latter of which must be surfaced to
    /// the user as an explicit unknown cost, not a silent (and previously wrong) dollar figure.
    pub fn is_recognized(&self, model: &str) -> bool {
        self.normalize_model_key(model).is_some()
    }

    /// Looks up the rate for (model, tier, kind) whose effective window contains `as_of`.
    /// Returns 0.0, never a substituted rate from a different model, when the model string --
    /// after normalizing away any provider-versioning suffix -- matches no row at all: an
    /// earlier version of this function fell back to `claude-3-5-sonnet`'s rate for any
    /// unrecognized model, which silently mispriced every real session using a model outside
    /// the seeded set (confirmed against real local session data: every current Claude, GPT, and
    /// Gemini model in use collapsed onto Sonnet's rate). Callers that need to distinguish this
    /// case from a genuinely free/zero-rate row must call [`is_recognized`] separately -- this
    /// function's 0.0 return alone is ambiguous between the two. When the model IS recognized
    /// but has no row for this specific (tier, kind, as_of), also returns 0.0 (CRIT-LUMEN-161).
    ///
    /// [`is_recognized`]: Self::is_recognized
    ///
    /// When a specific `tier` is requested but `lookup_model` has no row matching that exact
    /// tier for this (kind, as_of), falls back to `lookup_model`'s own `tier: None` row for the
    /// same (kind, as_of) before giving up (Blocker #2). seed() pushes every row with
    /// `tier: None`, so without this fallback any caller that passes a real tier (e.g.
    /// CodexAdapter's captured `service_tier`) matched zero rows and silently priced at 0.0. A
    /// row that matches the exact requested tier still always wins over this fallback.
    pub fn rate_for(&self, model: &str, tier: Option<&str>, kind: TokenRateKind, as_of: DateTime<Utc>) -> f64 {
        let lookup_model = self.normalize_model_key(model).unwrap_or(model);

        let matching = |wanted_tier: Option<&str>| {
            self.rates
                .iter()
                .filter(|r| {
                    r.model == lookup_model
                        && r.kind == kind
                        && r.tier.as_deref() == wanted_tier
                        && r.effective_from <= as_of
                        && r.effective_until.is_none_or(|until| as_of < until)
                })
                .max_by_key(|r| r.effective_from)
                .map(|r| r.rate_per_m)
        };

        matching(tier).or_else(|| if tier.is_some() { matching(None) } else { None }).unwrap_or(0.0)
    }
}
