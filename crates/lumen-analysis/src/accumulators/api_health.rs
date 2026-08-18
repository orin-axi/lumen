use lumen_model::CanonicalTurn;
use serde::{Deserialize, Serialize};

use crate::traits::EntryAccumulator;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApiHealthMetrics {
    pub rate_limit_429_count: u32,
    pub server_error_5xx_count: u32,
    pub retry_count: u32,
    pub total_error_events: u32,
}

#[derive(Debug, Default, Clone)]
pub struct ApiHealthAccumulator {
    pub rate_limit_429_count: u32,
    pub server_error_5xx_count: u32,
    pub retry_count: u32,
}

impl EntryAccumulator for ApiHealthAccumulator {
    type Output = ApiHealthMetrics;

    fn update(&mut self, entry: &CanonicalTurn) {
        for res in &entry.tool_results {
            if res.is_error {
                if let Some(err_class) = &res.error_class {
                    if err_class.contains("429") || err_class.contains("overloaded") {
                        self.rate_limit_429_count += 1;
                        self.retry_count += 1;
                    } else if err_class.contains("500") || err_class.contains("503") || err_class.contains("504") {
                        self.server_error_5xx_count += 1;
                        self.retry_count += 1;
                    }
                }
            }
        }
    }

    fn finalize(self) -> Self::Output {
        let total_error_events = self.rate_limit_429_count + self.server_error_5xx_count;
        ApiHealthMetrics {
            rate_limit_429_count: self.rate_limit_429_count,
            server_error_5xx_count: self.server_error_5xx_count,
            retry_count: self.retry_count,
            total_error_events,
        }
    }
}
