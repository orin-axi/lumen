use lumen_model::CanonicalTurn;
use serde::Serialize;

/// Lifecycle contract for streaming accumulators processing low-level JSON envelopes.
pub trait RawMessageAccumulator {
    type Output: Serialize + Send;
    fn update_raw(&mut self, message: &serde_json::Value);
    fn finalize(self) -> Self::Output;
}

/// Lifecycle contract for streaming accumulators processing normalized `CanonicalTurns`.
pub trait EntryAccumulator {
    type Output: Serialize + Send;
    fn update(&mut self, entry: &CanonicalTurn);
    fn finalize(self) -> Self::Output;
}
