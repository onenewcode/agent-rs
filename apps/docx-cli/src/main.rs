use agent_app::{AppConfig, AppContainer};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(short, long, default_value = "agent.toml")]
    config: PathBuf,
}

#[derive(Subcommand)]
enum Commands {
    /// Expand a DOCX document based on a prompt and optional URLs
    Run {
        #[arg(short, long)]
        doc: PathBuf,

        #[arg(short, long)]
        prompt: String,

        #[arg(short, long)]
        url: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();
    
    let config = AppConfig::from_path(&cli.config)
        .map_err(|e| anyhow::anyhow!("Config error: {}", e))?;
        
    let container = AppContainer::from_config(&config)
        .map_err(|e| anyhow::anyhow!("App initialization failed: {}", e))?;

    match cli.command {
        Commands::Run { doc, prompt, url } => {
            tracing::info!(path = ?doc, "Parsing document");
            let document = container.parse_doc(&doc)
                .map_err(|e| anyhow::anyhow!("Document parse failed: {}", e))?;

            tracing::info!(urls = ?url, "Fetching additional sources");
            let mut search_results = Vec::new();
            for u in url {
                let source = container.fetcher.fetch(&u).await
                    .map_err(|e| anyhow::anyhow!("Failed to fetch {}: {}", u, e))?;
                search_results.push(source);
            }

            tracing::info!("Starting expansion process");
            let initial_text = document.blocks.into_iter().map(|b| b.text).collect::<Vec<_>>().join("\n\n");
            let (report, final_doc) = container.run_expansion(prompt, initial_text).await
                .map_err(|e| anyhow::anyhow!("Expansion process failed: {}", e))?;

            tracing::info!(
                run_id = report.run_id,
                duration_ms = report.total_duration_ms,
                "Expansion complete"
            );
            
            println!("\n--- EXPANDED DOCUMENT ---\n");
            println!("{}", final_doc);
            println!("\n--- END ---\n");
        }
    }

    Ok(())
}
