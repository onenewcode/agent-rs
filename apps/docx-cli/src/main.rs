use std::{path::PathBuf, sync::Arc};

use agent_core::{ExpansionRuntime, Pipeline};
use anyhow::Context;
use clap::{Parser, Subcommand};
use docx_agent::DocxExpansionService;
use docx_agent::steps::{EvaluationStep, GenerationStep, RefinementStep, ResearchStep};
use evaluator_agent::EvaluatorService;
use orchestrator::AgentOrchestrator;
use rig::client::CompletionClient;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "docx-cli")]
#[command(about = "Expand DOCX documents with OpenRouter and optional Tavily search")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Expand(ExpandArgs),
}

#[derive(Debug, Parser)]
struct ExpandArgs {
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    doc: PathBuf,
    #[arg(long)]
    prompt: String,
    #[arg(long = "url")]
    urls: Vec<String>,
    #[arg(long, value_name = "PATH")]
    output: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing()?;
    let cli = Cli::parse();

    match cli.command {
        Commands::Expand(args) => expand(args).await?,
    }

    Ok(())
}

async fn expand(args: ExpandArgs) -> anyhow::Result<()> {
    let config_path = args.config.unwrap_or_else(default_config_path);
    info!(
        config = %config_path.display(),
        doc = %args.doc.display(),
        output = args.output.as_ref().map(|path| path.display().to_string()),
        urls = args.urls.len(),
        "starting document expansion"
    );

    // 1. Initialize core services
    let expansion_service = Arc::new(
        DocxExpansionService::from_config_path(&config_path)
            .with_context(|| format!("failed to load config {}", config_path.display()))?,
    );

    let config = expansion_service.config().clone();
    let http = reqwest::Client::builder()
        .user_agent(&config.fetch.user_agent)
        .build()?;

    // 2. Initialize the Evaluator
    let eval_client = rig::providers::openrouter::Client::builder()
        .api_key(config.evaluator.llm.api_key.as_str())
        .http_client(http.clone())
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build evaluator client: {e}"))?;

    let eval_agent = eval_client
        .agent(&config.evaluator.llm.model)
        .preamble(config.evaluator.system_prompt())
        .build();

    let evaluator_service = Arc::new(EvaluatorService::new(
        eval_agent,
        config.evaluator.evaluation_template().to_owned(),
        config.evaluator.max_attempts(),
    ));

    // 3. Construct the Pipeline
    let mut pipeline = Pipeline::new();
    pipeline.add_step(Box::new(ResearchStep::new(
        expansion_service.clone(),
        config.limits.global_timeout_secs,
    )));
    pipeline.add_step(Box::new(GenerationStep::new(expansion_service.clone())));
    pipeline.add_step(Box::new(EvaluationStep::new(
        evaluator_service.clone(),
        config.limits.min_score,
    )));

    // 4. Initialize the Orchestrator
    let refiner = Arc::new(RefinementStep::new(
        config.evaluator.refinement_template().to_owned(),
    ));
    let orchestrator =
        AgentOrchestrator::new(pipeline, Some(refiner), config.evaluator.max_attempts());

    // 5. Run the Pipeline
    let document = expansion_service
        .parse_document(&args.doc)
        .map_err(|e| anyhow::anyhow!("failed to parse document: {e}"))?;

    let request = agent_core::ExpansionRequest {
        prompt: args.prompt.clone(),
        document,
        user_urls: args.urls.clone(),
    };

    let result = orchestrator
        .expand(request)
        .await
        .with_context(|| format!("failed to expand document {}", args.doc.display()))?;

    // 6. Handle Results
    if result.is_qualified {
        info!(
            score = result.score,
            reason = result.evaluation_reason.as_deref().unwrap_or(""),
            "Document expansion qualified"
        );
        if let Some(output) = args.output {
            tokio::fs::write(&output, result.content)
                .await
                .with_context(|| format!("failed to write output file {}", output.display()))?;
            info!(output = %output.display(), "wrote expansion output");
        } else {
            println!("{}", result.content);
        }
    } else {
        error!(
            score = result.score,
            reason = result.evaluation_reason.as_deref().unwrap_or(""),
            "Document expansion FAILED qualification"
        );
        anyhow::bail!(
            "Generated content did not meet the quality threshold (Score: {}, Min: {})",
            result.score,
            config.limits.min_score
        );
    }

    Ok(())
}

fn init_tracing() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("docx_agent=info,docx_cli=info,rig=info"))?;
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize tracing: {error}"))?;
    Ok(())
}

fn default_config_path() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let primary = root.join("agent.toml");
    if primary.exists() {
        return primary;
    }

    let fallback = root.join("agent.example.toml");
    if fallback.exists() {
        info!(
            fallback = %fallback.display(),
            "agent.toml not found, falling back to example configuration"
        );
        return fallback;
    }

    primary
}
