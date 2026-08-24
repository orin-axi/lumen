use lumen_model::{CanonicalTranscript, TrajectoryAnomaly};
use smallvec::SmallVec;

use crate::accumulators::CircuitBreakerAccumulator;
use crate::traits::EntryAccumulator;

/// Detects `CanonicalTranscript.detected_anomalies` from a transcript's own `turns` --
/// CRIT-LUMEN-179. Every adapter hard-codes `detected_anomalies: SmallVec::new()` at parse
/// time (no adapter can detect a cross-turn pattern while streaming one line at a time), so
/// this is a caller-invoked post-processing step, not something adapters do themselves.
///
/// Covers 2 of `TrajectoryAnomaly`'s 4 variants:
/// - `CircularLoop`, via `lumen_pattern::detect_circular_loops` (Tarjan SCC over the tool-call
///   graph -- already real, tested, previously unreachable from any CLI command).
/// - `GateStall`, via `CircuitBreakerAccumulator` (already wired into `AnalyticsEngine`'s
///   per-turn pass, but that engine itself was never invoked from `lumen-cli` either).
///
/// Does NOT cover `ContextFlood` (needs a real flood threshold decision, not just wiring
/// existing data) or `UngroundedDrafting` (no supporting detector exists at all) -- both left
/// as documented, deliberately deferred gaps rather than guessed at here.
pub fn detect_trajectory_anomalies(transcript: &CanonicalTranscript) -> SmallVec<[TrajectoryAnomaly; 4]> {
    let mut anomalies = SmallVec::new();

    for loop_anomaly in lumen_pattern::detect_circular_loops(transcript) {
        anomalies.push(TrajectoryAnomaly::CircularLoop {
            symbol: loop_anomaly.symbol,
            cycle_depth: loop_anomaly.cycle_depth,
        });
    }

    let mut circuit_breaker = CircuitBreakerAccumulator::default();
    for turn in &transcript.turns {
        circuit_breaker.update(turn);
    }
    for stall in circuit_breaker.finalize().stalls {
        anomalies.push(TrajectoryAnomaly::GateStall {
            agent_pair: stall.agent_pair,
            observed_rounds: stall.observed_rounds,
        });
    }

    anomalies
}
