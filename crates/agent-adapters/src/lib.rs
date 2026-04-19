#![allow(clippy::missing_errors_doc)]

mod config;
mod fetch;
mod llm;
mod search;
mod stages;

pub use config::{
    AppConfig, CacheConfig, DocxConfig, GenerationConfig, LlmProviderConfig, ObservabilityConfig,
    ProviderConfig, ResearchConfig, RuntimeConfig, SearchConfig,
};
pub use fetch::{DiskCacheSourceFetcher, WebPageSourceFetcher};
pub use llm::{OpenRouterModel, build_openrouter_model};
pub use search::TavilySearchProvider;
pub use stages::{DocxEvaluator, DocxGenerator, DocxPlanner, DocxRefiner};
