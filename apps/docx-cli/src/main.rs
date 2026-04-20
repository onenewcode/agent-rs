use std::path::PathBuf;

use agent_app::PlatformApp;
use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use docx_domain::{DocxExpandRequest, DocxSourcePolicy};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "docx-cli")]
#[command(about = "Run multi-agent DOCX expansion jobs")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Run(RunArgs),
}

#[derive(Debug, Args)]
struct RunArgs {
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(long, default_value = "docx.expand")]
    workflow: String,
    #[arg(long, value_name = "PATH")]
    doc: PathBuf,
    #[arg(long)]
    prompt: String,
    #[arg(long = "url")]
    urls: Vec<String>,
    #[arg(long, value_name = "PATH")]
    output: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    disable_research: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing()?;
    let cli = Cli::parse();

    match cli.command {
        Commands::Run(args) => run(args).await?,
    }

    Ok(())
}

async fn run(args: RunArgs) -> anyhow::Result<()> {
    let config_path = args.config.clone().unwrap_or_else(default_config_path);
    let app = PlatformApp::from_path(&config_path)
        .with_context(|| format!("failed to load config {}", config_path.display()))?;

    info!(
        config = %config_path.display(),
        doc = %args.doc.display(),
        urls = args.urls.len(),
        "starting multi-agent expansion run"
    );

    let (report, final_doc) = app
        .run_docx(DocxExpandRequest {
            document_path: args.doc.display().to_string(),
            prompt: args.prompt.clone(),
            user_urls: args.urls.clone(),
            source_policy: DocxSourcePolicy {
                disable_research: args.disable_research,
            },
        })
        .await
        .with_context(|| format!("failed to execute agent task for {}", args.doc.display()))?;

    if report.qualified {
        info!("Multi-agent task qualified and complete.");
        if let Some(path) = args.output {
            tokio::fs::write(&path, &final_doc)
                .await
                .with_context(|| format!("failed to write output file {}", path.display()))?;
            info!(output = %path.display(), "wrote final document");
        } else {
            println!("{final_doc}");
        }
    } else {
        error!("Multi-agent task failed to meet quality threshold.");
        println!("Final document state anyway:\n{final_doc}");
    }

    println!("\n--- Run Summary ---");
    println!("Run ID: {}", report.run_id);
    println!("Tokens: {:?}", report.telemetry.usage);

    Ok(())
}

fn init_tracing() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().or_else(|_| {
        EnvFilter::try_new("agent_kernel=info,agent_runtime=info,agent_adapters=info,docx_domain=info,docx_cli=info,rig=info")
    })?;
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
