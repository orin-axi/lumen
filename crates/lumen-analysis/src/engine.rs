use compact_str::CompactString;
use lumen_model::{CanonicalTranscript, SchemaCitation};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::accumulators::{
    ApiHealthAccumulator, ApiHealthMetrics, ArtifactMetrics, ArtifactsAccumulator, AttributionAccumulator,
    AttributionMetrics, AutonomyAccumulator, AutonomyMetrics, CircuitBreakerAccumulator, CircuitBreakerReport,
    ContextGrowthAccumulator, ContextGrowthMetrics, FlowAccumulator, FlowMetrics, McpAffinityAccumulator,
    McpAffinityMetrics, PermissionMetrics, PermissionModeAccumulator, SchemaExtractorAccumulator,
    SelfCorrectionAccumulator, SelfCorrectionMetrics, StatsAccumulator, StatsMetrics, ToolInventoryAccumulator,
    ToolInventoryMetrics, TurnDurationAccumulator, TurnDurationMetrics,
};
use crate::traits::EntryAccumulator;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisReport {
    pub session_id: String,
    pub circuit_breaker: CircuitBreakerReport,
    pub turn_durations: TurnDurationMetrics,
    pub api_health: ApiHealthMetrics,
    pub mcp_affinity: McpAffinityMetrics,
    pub self_corrections: SelfCorrectionMetrics,
    pub context_growth: ContextGrowthMetrics,
    pub tool_inventory: ToolInventoryMetrics,
    pub autonomy: AutonomyMetrics,
    pub permission_mode: PermissionMetrics,
    pub artifacts: ArtifactMetrics,
    pub flow: FlowMetrics,
    pub stats: StatsMetrics,
    pub schema_extractor: Vec<SchemaCitation>,
    pub attribution: AttributionMetrics,
    pub by_subagent: BTreeMap<CompactString, AttributionMetrics>,
}

pub struct AnalyticsEngine {
    circuit_breaker: CircuitBreakerAccumulator,
    turn_duration: TurnDurationAccumulator,
    api_health: ApiHealthAccumulator,
    mcp_affinity: McpAffinityAccumulator,
    self_correction: SelfCorrectionAccumulator,
    context_growth: ContextGrowthAccumulator,
    tool_inventory: ToolInventoryAccumulator,
    autonomy: AutonomyAccumulator,
    permission_mode: PermissionModeAccumulator,
    artifacts: ArtifactsAccumulator,
    flow: FlowAccumulator,
    stats: StatsAccumulator,
    schema_extractor: SchemaExtractorAccumulator,
    attribution: AttributionAccumulator,
}

impl Default for AnalyticsEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalyticsEngine {
    pub fn new() -> Self {
        Self {
            circuit_breaker: CircuitBreakerAccumulator::default(),
            turn_duration: TurnDurationAccumulator::default(),
            api_health: ApiHealthAccumulator::default(),
            mcp_affinity: McpAffinityAccumulator::default(),
            self_correction: SelfCorrectionAccumulator::default(),
            context_growth: ContextGrowthAccumulator::default(),
            tool_inventory: ToolInventoryAccumulator::default(),
            autonomy: AutonomyAccumulator::default(),
            permission_mode: PermissionModeAccumulator::default(),
            artifacts: ArtifactsAccumulator::default(),
            flow: FlowAccumulator::default(),
            stats: StatsAccumulator::default(),
            schema_extractor: SchemaExtractorAccumulator::default(),
            attribution: AttributionAccumulator::default(),
        }
    }

    /// Single-pass execution of all accumulators over the canonical transcript.
    pub fn process_transcript(mut self, transcript: &CanonicalTranscript) -> AnalysisReport {
        for turn in &transcript.turns {
            self.circuit_breaker.update(turn);
            self.turn_duration.update(turn);
            self.api_health.update(turn);
            self.mcp_affinity.update(turn);
            self.self_correction.update(turn);
            self.context_growth.update(turn);
            self.tool_inventory.update(turn);
            self.autonomy.update(turn);
            self.permission_mode.update(turn);
            self.artifacts.update(turn);
            self.flow.update(turn);
            self.stats.update(turn);
            self.schema_extractor.update(turn);
            self.attribution.update(turn);
        }

        let mut by_subagent = BTreeMap::new();
        for subagent in &transcript.subagents {
            let sub_report = AnalyticsEngine::new().process_transcript(subagent);
            by_subagent.insert(CompactString::from(subagent.session_id.as_str()), sub_report.attribution);
        }

        AnalysisReport {
            session_id: transcript.session_id.to_string(),
            circuit_breaker: self.circuit_breaker.finalize(),
            turn_durations: self.turn_duration.finalize(),
            api_health: self.api_health.finalize(),
            mcp_affinity: self.mcp_affinity.finalize(),
            self_corrections: self.self_correction.finalize(),
            context_growth: self.context_growth.finalize(),
            tool_inventory: self.tool_inventory.finalize(),
            autonomy: self.autonomy.finalize(),
            permission_mode: self.permission_mode.finalize(),
            artifacts: self.artifacts.finalize(),
            flow: self.flow.finalize(),
            stats: self.stats.finalize(),
            schema_extractor: self.schema_extractor.finalize(),
            attribution: self.attribution.finalize(),
            by_subagent,
        }
    }
}
