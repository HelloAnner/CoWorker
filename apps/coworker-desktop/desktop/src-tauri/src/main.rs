#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--mcp-sidecar") {
        let mut config = None;
        let mut run_id = None;
        let mut sidecar_token = None;
        let mut ipc_port = None;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--config" => config = args.next(),
                "--run-id" => run_id = args.next(),
                "--sidecar-token" => sidecar_token = args.next(),
                "--ipc-port" => ipc_port = args.next().and_then(|value| value.parse::<u16>().ok()),
                _ => {}
            }
        }
        let config = config.unwrap_or_else(|| "coworker_desktop.json".to_owned());
        let run_id = run_id.unwrap_or_default();
        let sidecar_token = sidecar_token.unwrap_or_default();
        if run_id.is_empty() || sidecar_token.is_empty() || ipc_port.is_none() {
            eprintln!("--run-id, --sidecar-token, and --ipc-port are required for the MCP sidecar");
            std::process::exit(2);
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("create MCP sidecar runtime");
        let _ = config;
        if let Err(error) = runtime.block_on(coworker_desktop_core::mcp_sidecar::run_proxy(
            ipc_port.expect("checked ipc port"),
            &sidecar_token,
        )) {
            eprintln!("MCP sidecar failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    coworker_desktop_app_lib::run();
}
