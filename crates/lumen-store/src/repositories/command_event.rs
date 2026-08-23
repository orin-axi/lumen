use rusqlite::{params, Connection};

use crate::error::StoreError;
use crate::models::{CommandEventFactRecord, CommandEventReadModel};

/// Known secret-token prefixes (GitHub PATs, OpenAI/Anthropic-style `sk-`, AWS access keys,
/// Slack `xox` tokens, GitLab PATs, npm tokens, Google OAuth/API keys, JWTs). Matched as a fast,
/// unambiguous path before falling back to entropy -- real prior art from
/// gitleaks/ripsecrets/trufflehog's rule sets.
const SECRET_PREFIXES: &[&str] = &[
    "sk-", "pk-", "ghp_", "gho_", "ghu_", "ghs_", "ghr_", "xox", "AKIA", "ASIA", "AIza", "glpat-", "npm_", "ya29.",
    "eyJ",
];

/// Below this length, entropy is too noisy a signal to trust (a 4-character flag like `-xyz` can
/// trivially look "random") and real secrets are essentially never this short.
const MIN_SECRET_CANDIDATE_LEN: usize = 8;

/// Shannon entropy in bits/char above which a token's *shape* looks like a generated secret
/// rather than a human-chosen identifier or flag name. Calibrated against gitleaks' default
/// (~3.0-4.5 bits/char depending on charset); real flag/word tokens (`force-rebuild-all`,
/// `api-key`) sit well below this, real secrets (`MySecretPassword123`, base64/hex blobs) sit at
/// or above it.
const SECRET_ENTROPY_THRESHOLD: f64 = 3.5;

/// Shannon entropy of `s`, in bits per character. Higher entropy means less predictable -- the
/// same signal gitleaks/ripsecrets/trufflehog use to flag likely secrets by character shape
/// rather than position or a fixed prefix convention.
fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let len = s.chars().count() as f64;
    let mut counts: std::collections::HashMap<char, u32> = std::collections::HashMap::new();
    for c in s.chars() {
        *counts.entry(c).or_insert(0) += 1;
    }
    counts.values().fold(0.0, |acc, &count| {
        let p = f64::from(count) / len;
        acc - p * p.log2()
    })
}

/// Decides whether a token's *value* (a flag/dash prefix already stripped) looks like a secret
/// by shape, not by position -- the fix for CRIT-LUMEN-037: `starts with '-'` is a CLI-parsing
/// convention, not a security boundary, and let real secrets like `-sk-live-abc123SECRET` or
/// `-pMySecretPassword123` through unredacted.
fn looks_like_secret(candidate: &str) -> bool {
    if candidate.chars().count() < MIN_SECRET_CANDIDATE_LEN {
        return false;
    }
    if SECRET_PREFIXES.iter().any(|prefix| candidate.starts_with(prefix)) {
        return true;
    }
    shannon_entropy(candidate) >= SECRET_ENTROPY_THRESHOLD
}

pub struct CommandEventRepository<'a> {
    conn: &'a Connection,
}

impl<'a> CommandEventRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Redacts a raw (or already partially-redacted) argument string into a stable pattern:
    /// flag *names* are preserved (they're part of the command's fixed vocabulary, not private
    /// data -- e.g. `-m`, `--verbose`), but values are replaced with a fixed placeholder -- bare
    /// positional tokens outright, the value half of `--flag=value` pairs unconditionally, and
    /// any dash-prefixed token whose value shape looks like a secret (CRIT-LUMEN-037: entropy or
    /// a known secret prefix, not merely "starts with a dash"). This is the store's own
    /// redaction pass: `insert_command_events` never trusts a caller-supplied string to already
    /// be safe, since this repository is the last boundary before the argument string is written
    /// to disk. Tokenized with `shlex` (real shell-word splitting, so quoted multi-word
    /// arguments split correctly) rather than a naive whitespace split; if the input isn't valid
    /// shell syntax (e.g. an unbalanced quote), it falls back to a whitespace split so a
    /// malformed string still gets fully redacted instead of erroring out unsanitized.
    fn redact_args(raw: &str) -> String {
        let tokens: Vec<String> =
            shlex::split(raw).unwrap_or_else(|| raw.split_whitespace().map(str::to_string).collect());

        tokens
            .into_iter()
            .map(|token| {
                if let Some(long_flag) = token.strip_prefix("--") {
                    match long_flag.split_once('=') {
                        Some((name, _value)) => format!("--{name}=<redacted>"),
                        None if looks_like_secret(long_flag) => "<redacted>".to_string(),
                        None => token,
                    }
                } else if let Some(short_flag) = token.strip_prefix('-') {
                    if !short_flag.is_empty() && looks_like_secret(short_flag) {
                        "<redacted>".to_string()
                    } else {
                        token
                    }
                } else {
                    "<redacted>".to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn resolve_session_id(&self, provider: &str, session_id: &str) -> Result<i64, StoreError> {
        self.conn
            .query_row(
                "SELECT id FROM sessions WHERE provider = ?1 AND provider_session_id = ?2",
                params![provider, session_id],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound(format!(
                    "session with provider '{provider}' and provider_session_id '{session_id}' not found"
                )),
                other => StoreError::Sqlite(other),
            })
    }

    pub fn insert_command_events(
        &self,
        provider: &str,
        session_id: &str,
        events: &[CommandEventFactRecord],
    ) -> Result<(), StoreError> {
        let internal_id = self.resolve_session_id(provider, session_id)?;

        let mut stmt = self
            .conn
            .prepare(
                "INSERT INTO command_events (session_id, command_base, sanitized_args, is_error)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(StoreError::Sqlite)?;

        for e in events {
            let redacted = e.sanitized_args.as_deref().map(Self::redact_args);
            stmt.execute(params![internal_id, e.command_base, redacted, if e.is_error { 1 } else { 0 },])
                .map_err(StoreError::Sqlite)?;
        }

        Ok(())
    }

    pub fn list_by_session(&self, provider: &str, session_id: &str) -> Result<Vec<CommandEventReadModel>, StoreError> {
        let internal_id = self.resolve_session_id(provider, session_id)?;

        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, session_id, command_base, sanitized_args, is_error
                 FROM command_events
                 WHERE session_id = ?1
                 ORDER BY id ASC",
            )
            .map_err(StoreError::Sqlite)?;

        let rows = stmt
            .query_map(params![internal_id], |row| {
                let is_error: i64 = row.get(4)?;
                Ok(CommandEventReadModel {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    command_base: row.get(2)?,
                    sanitized_args: row.get(3)?,
                    is_error: is_error != 0,
                })
            })
            .map_err(StoreError::Sqlite)?;

        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(StoreError::Sqlite)?);
        }
        Ok(result)
    }
}
