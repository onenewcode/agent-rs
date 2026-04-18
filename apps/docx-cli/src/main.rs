use std::{fs, path::PathBuf};

use anyhow::Context;
use clap::{Parser, Subcommand};
use docx_agent::DocxExpansionService;
use tracing::info;
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
    let service = DocxExpansionService::from_config_path(&config_path)
        .with_context(|| format!("failed to load config {}", config_path.display()))?;
    let result = service
        .expand_file(&args.doc, &args.prompt, &args.urls)
        .await
        .with_context(|| format!("failed to expand document {}", args.doc.display()))?;

    if let Some(output) = args.output {
        fs::write(&output, result.content)
            .with_context(|| format!("failed to write output file {}", output.display()))?;
        info!(output = %output.display(), "wrote expansion output");
    } else {
        println!("{}", result.content);
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
