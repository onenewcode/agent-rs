use std::{path::PathBuf, sync::Arc};

use agent_adapters::{
    AppConfig, DiskCacheSourceFetcher, DocxEvaluator, DocxGenerator, DocxPlanner, DocxRefiner,
    TavilySearchProvider, WebPageSourceFetcher, build_openrouter_model,
};
use agent_kernel::{DocumentParser, LanguageModel, RunConstraints, RunReport, Task};
use agent_runtime::{AgentRuntime, DefaultResearcher, ResearchSettings, RuntimeSettings};
use anyhow::Context;
use clap::{Parser, Subcommand};
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
    let app_config = AppConfig::from_path(&config_path)
        .with_context(|| format!("failed to load config {}", config_path.display()))?;
    info!(
        config = %config_path.display(),
        doc = %args.doc.display(),
        output = args.output.as_ref().map(|path| path.display().to_string()),
        urls = args.urls.len(),
        "starting document expansion"
    );

    let runtime = build_runtime(&app_config)?;
    let report = run_task(&runtime, &args)
        .await
        .with_context(|| format!("failed to expand document {}", args.doc.display()))?;
    handle_report(&app_config, report, args.output).await?;

    Ok(())
}

fn build_runtime(app_config: &AppConfig) -> anyhow::Result<AgentRuntime> {
    let http = reqwest::Client::builder()
        .user_agent(&app_config.observability.user_agent)
        .build()?;
    let formatter = app_config.prompt_formatter();
    let system_prompt = formatter.system_prompt().to_owned();

    let planner_model = build_model(http.clone(), app_config.providers.generator.clone(), &system_prompt)?;
    let generator_model =
        build_model(http.clone(), app_config.providers.generator.clone(), &system_prompt)?;
    let evaluator_model =
        build_model(http.clone(), app_config.providers.evaluator.clone(), &system_prompt)?;
    let refiner_model = build_model(http.clone(), app_config.providers.generator.clone(), &system_prompt)?;

    let search_provider = build_search_provider(app_config, &http)?;
    let fetcher = Arc::new(DiskCacheSourceFetcher::new(
        WebPageSourceFetcher::new(
            http,
            app_config.generation.source_tokens * 4,
            app_config.observability.fetch_timeout_secs,
        ),
        &app_config.cache.dir,
        app_config.cache.max_age_days,
    )) as Arc<dyn agent_kernel::SourceFetcher>;

    AgentRuntime::builder(RuntimeSettings {
        min_score: app_config.runtime.min_score,
        global_timeout_secs: app_config.runtime.global_timeout_secs,
    })
    .with_planner(Arc::new(DocxPlanner::new(
        Some(planner_model),
        formatter.clone(),
        &app_config.research,
        app_config.runtime.max_refinement_rounds,
    )))
    .with_researcher(Arc::new(DefaultResearcher::new(
        fetcher,
        search_provider,
        ResearchSettings {
            search_max_results: app_config.research.max_search_results,
            fetch_concurrency_limit: app_config.research.fetch_concurrency_limit,
        },
    )))
    .with_generator(Arc::new(DocxGenerator::new(generator_model, formatter.clone())))
    .with_evaluator(Arc::new(DocxEvaluator::new(evaluator_model, formatter.clone())))
    .with_refiner(Arc::new(DocxRefiner::new(refiner_model, formatter)))
    .build()
    .map_err(Into::into)
}

fn build_model(
    http: reqwest::Client,
    config: agent_adapters::LlmProviderConfig,
    system_prompt: &str,
) -> anyhow::Result<Arc<dyn LanguageModel>> {
    Ok(Arc::new(build_openrouter_model(
        http,
        config,
        system_prompt.to_owned(),
    )?))
}

fn build_search_provider(
    app_config: &AppConfig,
    http: &reqwest::Client,
) -> anyhow::Result<Option<Arc<dyn agent_kernel::SearchProvider>>> {
    let Some(search) = &app_config.providers.search else {
        return Ok(None);
    };

    if search.provider != "tavily" {
        anyhow::bail!("unsupported search provider `{}`", search.provider);
    }

    Ok(Some(Arc::new(TavilySearchProvider::new(
        http.clone(),
        &search.api_key,
        app_config.generation.source_tokens * 4,
        app_config.observability.search_timeout_secs,
    )) as Arc<dyn agent_kernel::SearchProvider>))
}

async fn run_task(runtime: &AgentRuntime, args: &ExpandArgs) -> anyhow::Result<RunReport> {
    let document = docx_domain::DocxDocumentParser
        .parse_path(&args.doc)
        .map_err(|error| anyhow::anyhow!("failed to parse document: {error}"))?;

    runtime
        .run(Task {
            prompt: args.prompt.clone(),
            document,
            user_urls: args.urls.clone(),
            constraints: RunConstraints::default(),
        })
        .await
        .map_err(Into::into)
}

async fn handle_report(
    app_config: &AppConfig,
    report: RunReport,
    output: Option<PathBuf>,
) -> anyhow::Result<()> {
    if report.qualified {
        info!(
            score = report.final_score,
            reason = report.final_reason.as_deref().unwrap_or(""),
            attempts = report.attempts.len(),
            "document expansion qualified"
        );
        if let Some(output) = output {
            tokio::fs::write(&output, &report.final_output)
                .await
                .with_context(|| format!("failed to write output file {}", output.display()))?;
            info!(output = %output.display(), "wrote expansion output");
        } else {
            println!("{}", report.final_output);
        }
        return Ok(());
    }

    error!(
        score = report.final_score,
        reason = report.final_reason.as_deref().unwrap_or(""),
        attempts = report.attempts.len(),
        "document expansion failed qualification"
    );
    anyhow::bail!(
        "Generated content did not meet the quality threshold (Score: {}, Min: {})",
        report.final_score,
        app_config.runtime.min_score
    )
}

fn init_tracing() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("agent_runtime=info,docx_cli=info,rig=info"))?;
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
