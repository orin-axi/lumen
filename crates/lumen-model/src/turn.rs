use chrono::{DateTime, Utc};
use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

/// A single semantic turn within an agent execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalTurn {
    pub turn_index: usize,
    pub role: TurnRole,
    pub timestamp: DateTime<Utc>,
    pub latency_ms: u64,
    pub text: Option<String>,
    pub tool_calls: SmallVec<[CanonicalToolCall; 2]>,
    pub tool_results: SmallVec<[CanonicalToolResult; 2]>,
    pub usage: Option<TurnTokenUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnRole {
    User,
    Assistant,
    System,
    ToolResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalToolCall {
    pub call_id: CompactString,
    pub tool_name: CompactString,
    pub intent: ToolIntent,
    pub raw_arguments: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToolIntent {
    FileRead { path: CompactString, line_range: Option<(usize, usize)> },
    FileEdit { path: CompactString, lines_added: usize, lines_removed: usize },
    FileCreate { path: CompactString },
    CodeSearch { tool: CompactString, query: CompactString, is_ast: bool },
    FileDiscovery { tool: CompactString, pattern: CompactString },
    TestExecution { runner: CompactString, target_suite: Option<CompactString> },
    VersionControl { action: CompactString },
    SubagentSpawn { agent_type: CompactString, description: CompactString },
    McpCall { server: CompactString, method: CompactString },
    Other { raw_name: CompactString },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalToolResult {
    pub call_id: CompactString,
    pub output_bytes: usize,
    pub line_count: usize,
    pub is_error: bool,
    pub error_class: Option<CompactString>,
    pub truncated_output: Option<CompactString>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TurnTokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
}

impl TurnTokenUsage {
    #[inline]
    pub fn prompt_tokens(&self) -> u64 {
        self.input_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }

    #[inline]
    pub fn total_tokens(&self) -> u64 {
        self.prompt_tokens() + self.output_tokens
    }
}
