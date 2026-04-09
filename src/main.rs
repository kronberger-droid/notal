mod parser;
mod server;
mod vault;

use clap::Parser;
use rmcp::ServiceExt;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "notal", version, about = "Lean Obsidian vault MCP server")]
struct Cli {
    /// Path to the Obsidian vault directory
    #[arg(long, env = "NOTAL_VAULT_PATH")]
    vault_path: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "notal=info".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    // Expand ~ in vault path
    let expanded = shellexpand(&cli.vault_path);
    let vault_path = PathBuf::from(&expanded);

    if !vault_path.is_dir() {
        anyhow::bail!("vault path is not a directory: {}", vault_path.display());
    }

    let vault_path = vault_path.canonicalize()?;
    tracing::info!("notal serving vault: {}", vault_path.display());

    let server = server::Notal::new(vault_path);
    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;

    Ok(())
}

/// Simple tilde expansion without an extra dependency.
fn shellexpand(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return format!("{}/{}", home.to_string_lossy(), rest);
        }
    }
    path.to_string()
}
