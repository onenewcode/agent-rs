use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    #[serde(default = "LlmConfig::default_input_cost")]
    pub input_cost_per_1m: f64,
    #[serde(default = "LlmConfig::default_output_cost")]
    pub output_cost_per_1m: f64,
}

impl LlmConfig {
    #[must_use]
    pub fn default_input_cost() -> f64 {
        0.15
    }

    #[must_use]
    pub fn default_output_cost() -> f64 {
        0.60
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    pub provider: String,
    pub api_key: Option<String>,
    pub max_results: usize,
    #[serde(default = "SearchConfig::default_timeout_secs")]
    pub timeout_secs: u64,
}

impl SearchConfig {
    #[must_use]
    pub fn default_timeout_secs() -> u64 {
        30
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LimitsConfig {
    pub document_tokens: usize,
    pub source_tokens: usize,
    pub max_total_tokens: Option<usize>,
    #[serde(default = "LimitsConfig::default_global_timeout_secs")]
    pub global_timeout_secs: u64,
    #[serde(default = "LimitsConfig::default_min_score")]
    pub min_score: u8,
}

impl LimitsConfig {
    #[must_use]
    pub fn default_global_timeout_secs() -> u64 {
        180
    }

    #[must_use]
    pub fn default_min_score() -> u8 {
        80
    }

    #[must_use]
    pub fn max_total_tokens(&self) -> usize {
        self.max_total_tokens.unwrap_or(128_000)
    }
}
