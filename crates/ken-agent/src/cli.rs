//! CLI argument parsing for the Ken agent.
//!
//! The agent binary supports multiple modes of operation, dispatched
//! based on the first positional argument.

/// The action the agent should take based on CLI arguments.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Install the Windows service.
    Install,
    /// Uninstall the Windows service.
    Uninstall,
    /// Run as the Windows service (called by SCM).
    RunService,
    /// Run the user-mode Tray App.
    Tray,
    /// Perform enrollment against the given URL.
    Enroll { url: String },
    /// Print the agent's current status.
    Status,
    /// Trigger the local kill switch.
    KillSwitch,
    /// Show version and usage.
    Help,
}

/// Parse CLI arguments into an [`Action`].
pub fn parse_args(args: &[String]) -> Action {
    let subcommand = args.get(1).map_or("help", String::as_str);

    match subcommand {
        "install" => Action::Install,
        "uninstall" => Action::Uninstall,
        "run-service" => Action::RunService,
        "tray" => Action::Tray,
        "enroll" => {
            let url = args
                .iter()
                .position(|a| a == "--url")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_default();
            Action::Enroll { url }
        }
        "status" => Action::Status,
        "kill-switch" => Action::KillSwitch,
        "--help" | "-h" | "help" => Action::Help,
        _ => {
            eprintln!("unknown command: {subcommand}");
            Action::Help
        }
    }
}

/// Print usage information.
pub fn print_usage() {
    eprintln!(
        "\
Ken Agent — Windows endpoint observability for family IT

Usage: ken-agent <command>

Commands:
    install       Install the Ken Agent Windows service
    uninstall     Uninstall the Ken Agent Windows service
    run-service   Run as the Windows service (called by SCM)
    tray          Run the user-mode Tray App
    enroll        Enroll with a Ken server
                  --url <enrollment-url>
    status        Print the agent's current status
    kill-switch   Activate the local kill switch (stops service)
    help          Show this help message

https://github.com/t11z/ken"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_install() {
        let args = vec!["ken-agent".into(), "install".into()];
        assert_eq!(parse_args(&args), Action::Install);
    }

    #[test]
    fn parse_uninstall() {
        let args = vec!["ken-agent".into(), "uninstall".into()];
        assert_eq!(parse_args(&args), Action::Uninstall);
    }

    #[test]
    fn parse_enroll_with_url() {
        let args = vec![
            "ken-agent".into(),
            "enroll".into(),
            "--url".into(),
            "https://ken.local:8444/enroll/abc".into(),
        ];
        assert_eq!(
            parse_args(&args),
            Action::Enroll {
                url: "https://ken.local:8444/enroll/abc".into()
            }
        );
    }

    #[test]
    fn parse_enroll_without_url() {
        let args = vec!["ken-agent".into(), "enroll".into()];
        assert_eq!(parse_args(&args), Action::Enroll { url: String::new() });
    }

    #[test]
    fn parse_help() {
        let args = vec!["ken-agent".into(), "help".into()];
        assert_eq!(parse_args(&args), Action::Help);
    }

    #[test]
    fn parse_no_args_shows_help() {
        let args = vec!["ken-agent".into()];
        assert_eq!(parse_args(&args), Action::Help);
    }

    #[test]
    fn parse_run_service() {
        let args = vec!["ken-agent".into(), "run-service".into()];
        assert_eq!(parse_args(&args), Action::RunService);
    }

    #[test]
    fn parse_tray() {
        let args = vec!["ken-agent".into(), "tray".into()];
        assert_eq!(parse_args(&args), Action::Tray);
    }

    #[test]
    fn parse_kill_switch() {
        let args = vec!["ken-agent".into(), "kill-switch".into()];
        assert_eq!(parse_args(&args), Action::KillSwitch);
    }
}
