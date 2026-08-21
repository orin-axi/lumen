use compact_str::CompactString;
use lumen_model::CanonicalTranscript;
use serde::{Deserialize, Serialize};

pub struct OtelCorrelationAccumulator;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OtelCorrelationReport {
    pub session_id: CompactString,
    pub request_ids: Vec<CompactString>,
    pub request_id_count: usize,
}

impl OtelCorrelationAccumulator {
    pub fn finalize(transcript: &CanonicalTranscript) -> OtelCorrelationReport {
        let request_ids: Vec<CompactString> = transcript.otel_conversation_id.iter().cloned().collect();
        let request_id_count = request_ids.len();

        OtelCorrelationReport { session_id: transcript.session_id.clone(), request_ids, request_id_count }
    }
}
