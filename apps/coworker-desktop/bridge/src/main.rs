use std::{
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
};

use clap::Parser;
use coworker_desktop_core::{
    config::{
        DEFAULT_DESKTOP_CONFIG_PATH, codex_names_for_user_name, default_codex_names,
        default_config_value_with_display_name,
    },
    error::{BridgeError, Result},
    runtime::BridgeRuntime,
};

#[derive(Debug, Parser)]
#[command(
    name = "coworker-desktop",
    about = "Run CoWorker Desktop communication bridge"
)]
struct Args {
    #[arg(long, default_value = DEFAULT_DESKTOP_CONFIG_PATH)]
    config: PathBuf,
    #[arg(long)]
    mcp_sidecar: bool,
    #[arg(long)]
    run_id: Option<String>,
    #[arg(long)]
    sidecar_token: Option<String>,
    #[arg(long)]
    ipc_port: Option<u16>,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let args = Args::parse();
    if args.mcp_sidecar {
        let run_id = args
            .run_id
            .as_deref()
            .ok_or_else(|| BridgeError::Config("--run-id is required for MCP sidecar".into()))?;
        let token = args.sidecar_token.as_deref().ok_or_else(|| {
            BridgeError::Config("--sidecar-token is required for MCP sidecar".into())
        })?;
        let port = args
            .ipc_port
            .ok_or_else(|| BridgeError::Config("--ipc-port is required for MCP sidecar".into()))?;
        let _ = run_id;
        return coworker_desktop_core::mcp_sidecar::run_proxy(port, token).await;
    }
    ensure_bridge_config(&args.config)?;
    let runtime = BridgeRuntime::new();
    runtime.start(&args.config).await?;
    tokio::signal::ctrl_c().await?;
    runtime.stop().await?;
    Ok(())
}

fn ensure_bridge_config(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(BridgeError::MissingConfig(path.to_path_buf()));
    }
    create_bridge_config_interactively(path)
}

fn create_bridge_config_interactively(path: &Path) -> Result<()> {
    println!("CoWorker Desktop config was not found: {}", path.display());
    let mut default_names = default_codex_names();
    if !default_names.used_user_name {
        let user_name = prompt_optional("Your name for Codex defaults (optional): ")?;
        if let Some(user_name) = user_name {
            default_names = codex_names_for_user_name(&user_name);
        }
    }
    let codex_id = prompt_non_empty(
        &format!("Codex id [{}]: ", default_names.codex_id),
        &default_names.codex_id,
    )?;
    let display_name = if default_names.used_user_name || codex_id == default_names.codex_id {
        default_names.display_name
    } else {
        "Local Codex".to_owned()
    };
    let base_url = prompt_base_url()?;
    let data = default_config_value_with_display_name(&codex_id, &display_name, &base_url);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(&data)?)?;
    println!("Created CoWorker Desktop config: {}", path.display());
    println!(
        "Production mode is enabled by default; review HTTPS URLs and Bearer tokens before starting."
    );
    println!(
        "For local HTTP-only development, explicitly set security.development_mode=true in this file."
    );
    println!("Review the config, start Coworker, then rerun coworker-desktop.");
    Ok(())
}

fn prompt_optional(prompt: &str) -> Result<Option<String>> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let value = line.trim();
    Ok((!value.is_empty()).then(|| value.to_owned()))
}

fn prompt_non_empty(prompt: &str, default: &str) -> Result<String> {
    loop {
        print!("{prompt}");
        io::stdout().flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let value = line.trim();
        if value.is_empty() {
            return Ok(default.to_owned());
        }
        if !value.is_empty() {
            return Ok(value.to_owned());
        }
    }
}

fn prompt_base_url() -> Result<String> {
    loop {
        let value = prompt_non_empty(
            "Coworker base URL [https://localhost:8000]: ",
            "https://localhost:8000",
        )?;
        if value.starts_with("http://") || value.starts_with("https://") {
            return Ok(value.trim_end_matches('/').to_owned());
        }
        println!("Base URL must start with http:// or https://");
    }
}
