use compact_str::CompactString;
use serde::Serialize;
use std::collections::BTreeMap;

use crate::traits::RawMessageAccumulator;

#[derive(Debug, Default, Clone, Serialize)]
pub struct HookActivityAccumulator {
    pub hook_invocations: usize,
    pub by_event: BTreeMap<CompactString, usize>,
    pub total_duration_ms: u64,
    pub blocked_count: usize,
    pub block_rate: f64,
    pub avg_duration_ms: f64,
}

impl RawMessageAccumulator for HookActivityAccumulator {
    type Output = Self;

    fn update_raw(&mut self, message: &serde_json::Value) {
        if let Some(event_name) = message.get("hookEventName").and_then(|v| v.as_str()) {
            self.hook_invocations += 1;
            *self.by_event.entry(CompactString::new(event_name)).or_insert(0) += 1;
            self.total_duration_ms += message.get("durationMs").and_then(serde_json::Value::as_u64).unwrap_or(0);
        }

        let blocked = message.get("decision").and_then(|v| v.as_str()) == Some("block")
            || message.get("permissionDecision").and_then(|v| v.as_str()) == Some("deny");
        if blocked {
            self.blocked_count += 1;
        }
    }

    fn finalize(mut self) -> Self::Output {
        if self.hook_invocations > 0 {
            self.block_rate = self.blocked_count as f64 / self.hook_invocations as f64;
            self.avg_duration_ms = self.total_duration_ms as f64 / self.hook_invocations as f64;
        } else {
            self.block_rate = 0.0;
            self.avg_duration_ms = 0.0;
        }
        self
    }
}
