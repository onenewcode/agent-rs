use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Telemetry {
    pub usage: TokenUsage,
    pub estimated_cost_usd: f64,
}

impl Telemetry {
    pub fn add_usage(&mut self, model_id: &str, usage: TokenUsage) {
        self.usage.prompt_tokens += usage.prompt_tokens;
        self.usage.completion_tokens += usage.completion_tokens;
        self.usage.total_tokens += usage.total_tokens;

        // Cost estimation based on common OpenRouter pricing (per 1M tokens)
        let (prompt_rate, completion_rate) = match model_id {
            m if m.contains("gpt-4o-mini") => (0.15, 0.60),
            m if m.contains("gpt-4o") => (2.50, 10.00),
            m if m.contains("claude-3-5-sonnet") => (3.00, 15.00),
            m if m.contains("claude-3-haiku") => (0.25, 1.25),
            m if m.contains(":free") => (0.00, 0.00),
            _ => (0.50, 1.50), // Generic fallback
        };

        #[allow(clippy::cast_precision_loss)]
        let cost = (usage.prompt_tokens as f64 * prompt_rate / 1_000_000.0)
            + (usage.completion_tokens as f64 * completion_rate / 1_000_000.0);

        self.estimated_cost_usd += cost;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_cost_calculation() {
        let mut tel = Telemetry::default();
        let usage = TokenUsage {
            prompt_tokens: 1_000,
            completion_tokens: 500,
            total_tokens: 1_500,
        };

        // Test GPT-4o-mini
        tel.add_usage("gpt-4o-mini", usage);
        // (1000 * 0.15 / 1M) + (500 * 0.60 / 1M) = 0.00015 + 0.0003 = 0.00045
        assert!((tel.estimated_cost_usd - 0.00045).abs() < f64::EPSILON);

        // Test Claude 3.5 Sonnet
        let mut tel2 = Telemetry::default();
        tel2.add_usage("claude-3-5-sonnet", usage);
        // (1000 * 3.0 / 1M) + (500 * 15.0 / 1M) = 0.003 + 0.0075 = 0.0105
        assert!((tel2.estimated_cost_usd - 0.0105).abs() < f64::EPSILON);

        // Test Free model
        let mut tel3 = Telemetry::default();
        tel3.add_usage("model:free", usage);
        assert_eq!(tel3.estimated_cost_usd, 0.0);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrajectoryStep {
    Thought {
        text: String,
        usage: Option<TokenUsage>,
        duration_ms: Option<u64>,
    },
    Action {
        tool: String,
        input: Value,
        output: String,
        is_error: bool,
        duration_ms: Option<u64>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTrajectory {
    pub steps: Vec<TrajectoryStep>,
}
