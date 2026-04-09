//! Ken agent — Windows endpoint observability and consent-gated remote access.
//!
//! The agent runs as a Windows service under `LocalSystem` and reports
//! passive OS state (Defender, firewall, `BitLocker`, Windows Update,
//! security events) to the Ken server. A user-mode Tray App provides
//! visibility and the consent gate for remote sessions.
//!
// Many modules define types and functions that are prepared for use in later
// phases or on specific platforms. Suppress dead_code at the crate level so
// RUSTFLAGS=-D warnings in CI does not reject them.
#![allow(dead_code)]

mod audit;
mod cli;
mod config;
mod ipc;
mod killswitch;
mod observer;
mod remote_session;
mod service;
mod worker;

use cli::Action;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let action = cli::parse_args(&args);

    match action {
        Action::Install => {
            if let Err(e) = service::lifecycle::install_service() {
                eprintln!("failed to install service: {e}");
                std::process::exit(1);
            }
        }
        Action::Uninstall => {
            if let Err(e) = service::lifecycle::uninstall_service() {
                eprintln!("failed to uninstall service: {e}");
                std::process::exit(1);
            }
        }
        Action::RunService => {
            // On Windows, this would call windows_service::service_dispatcher.
            // On other platforms, run the service loop directly for development.
            #[cfg(windows)]
            {
                eprintln!("Windows service dispatch not yet implemented");
                std::process::exit(1);
            }
            #[cfg(not(windows))]
            {
                let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
                let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let shutdown_clone = shutdown.clone();

                ctrlc_handler(shutdown_clone);

                rt.block_on(service::lifecycle::service_loop(shutdown));
            }
        }
        Action::Tray => {
            eprintln!("Tray App not yet implemented (Section 8)");
        }
        Action::Enroll { url } => {
            if url.is_empty() {
                eprintln!("error: --url is required for enrollment");
                eprintln!("usage: ken-agent enroll --url <enrollment-url>");
                std::process::exit(1);
            }
            eprintln!("Enrollment not yet implemented (Section 10)");
            eprintln!("URL: {url}");
        }
        Action::Status => print_status(),
        Action::KillSwitch => activate_kill_switch(),
        Action::Help => {
            cli::print_usage();
        }
    }
}

/// Print the agent's current status to stdout.
fn print_status() {
    let data_dir = config::data_dir();
    let paths = config::DataPaths::new(&data_dir);

    println!("Ken Agent Status");
    println!("================");
    println!("Data directory: {}", data_dir.display());
    println!(
        "Config file: {} ({})",
        paths.config_file.display(),
        if paths.config_file.exists() {
            "exists"
        } else {
            "not found"
        }
    );
    println!(
        "Enrolled: {}",
        if paths.endpoint_id_file.exists() {
            "yes"
        } else {
            "no"
        }
    );
    println!(
        "Kill switch: {}",
        if paths.kill_switch_file.exists() {
            "ACTIVE"
        } else {
            "not active"
        }
    );
}

/// Activate the local kill switch (ADR-0001 T1-6, ADR-0012).
fn activate_kill_switch() {
    let data_dir = config::data_dir();
    let paths = config::DataPaths::new(&data_dir);

    let user = whoami();
    match killswitch::activate(&paths.kill_switch_file, "user requested via CLI", &user) {
        Ok(()) => {
            println!("Kill switch activated. The Ken Agent service will not start.");
            println!("To reverse, delete: {}", paths.kill_switch_file.display());
            println!("Then run: sc config KenAgent start= auto && sc start KenAgent");
        }
        Err(e) => {
            eprintln!("failed to activate kill switch: {e}");
            std::process::exit(1);
        }
    }
}

/// Get the current username for audit logging.
fn whoami() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Set up a Ctrl+C handler that sets the shutdown flag.
#[cfg(not(windows))]
fn ctrlc_handler(shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>) {
    let _ = ctrlc::set_handler(move || {
        shutdown.store(true, std::sync::atomic::Ordering::SeqCst);
    });
}
