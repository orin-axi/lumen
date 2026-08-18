use compact_str::CompactString;
use serde::{Deserialize, Serialize};

/// Citation and validation status of an extracted schema block (e.g. spec@1, plan@1, changeset@1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaCitation {
    /// Schema identifier (e.g. "spec@1", "plan@1", "changeset@1", "eval-report@1")
    pub schema_id: CompactString,
    /// Turn index where the schema was emitted
    pub turn_index: usize,
    /// Whether the extracted JSON validated against the Draft-07 schema
    pub is_valid: bool,
    /// Extracted title or goal summary if present
    pub summary: Option<CompactString>,
}
