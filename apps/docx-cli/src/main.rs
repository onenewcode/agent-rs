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
    /// Expand a DOCX document based on a prompt
    Run {
        #[arg(short, long)]
        doc: PathBuf,

        #[arg(short, long)]
        prompt: String,

        /// Optional path to save the expanded document. If not provided, it will be saved next to the input file.
        #[arg(short, long)]
        output: Option<PathBuf>,
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

    let config =
        AppConfig::from_path(&cli.config).map_err(|e| anyhow::anyhow!("Config error: {e}"))?;

    let container = AppContainer::from_config(&config)
        .map_err(|e| anyhow::anyhow!("App initialization failed: {e}"))?;

    match cli.command {
        Commands::Run {
            doc,
            prompt,
            output,
        } => {
            tracing::info!(path = ?doc, "Parsing document");
            let document = container
                .parse_doc(&doc)
                .map_err(|e| anyhow::anyhow!("Document parse failed: {e}"))?;

            tracing::info!("Starting expansion process (Autonomous Research Enabled)");
            let initial_text = document
                .blocks
                .into_iter()
                .map(|b| b.text)
                .collect::<Vec<_>>()
                .join("\n\n");
            let (report, final_doc) = container
                .run_expansion(prompt, initial_text)
                .await
                .map_err(|e| anyhow::anyhow!("Expansion process failed: {e}"))?;

            tracing::info!(
                run_id = report.run_id,
                duration_ms = report.total_duration_ms,
                "Expansion complete. Persisting run report..."
            );

            // Persist the full conversation history and telemetry
            container
                .storage
                .persist(&report)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to persist run report: {e}"))?;

            // Determine output path
            let output_path = output.unwrap_or_else(|| {
                let mut p = doc.clone();
                p.set_extension("expanded.md");
                p
            });

            tracing::info!(path = ?output_path, "Saving expanded document");
            tokio::fs::write(&output_path, &final_doc)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to save output document: {e}"))?;

            println!("\n--- EXPANDED DOCUMENT ---\n");
            println!("{final_doc}");
            println!("\n--- END ---\n");

            tracing::info!("Run report saved to {}", config.services.artifacts.dir);
            tracing::info!("Expanded document saved to {:?}", output_path);
        }
    }

    Ok(())
}
