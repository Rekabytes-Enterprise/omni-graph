mod app;
mod attachments;
mod cli;
mod database;
mod idempotency;
mod install;
mod integrations;
mod model;
mod preparation;
mod query;
mod retrieval;
mod storage;
mod transfer;
mod tui;
mod ui;
mod update;
mod upgrade;
mod web;

use clap::Parser;
mod compatibility;
mod compatibility_probe;

fn contract_error(error: compatibility::ContractError) -> ! {
    println!("{}", serde_json::json!({"error": error}));
    std::process::exit(error.exit_code);
}
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let explicit_api = args
        .iter()
        .any(|a| a == "--api-version" || a.to_string_lossy().starts_with("--api-version="));
    // Only the subcommand selects discovery, never a query or another option's value.
    let mut values = args.iter().skip(1);
    let mut discovery = false;
    while let Some(value) = values.next() {
        if value == "--directory" {
            values.next();
            continue;
        }
        if value.to_string_lossy().starts_with('-') {
            continue;
        }
        discovery = value == "compatibility" || value == "prepare";
        break;
    }
    let contract_requested = explicit_api || discovery;
    let cli = match cli::Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) if contract_requested && error.use_stderr() => {
            contract_error(compatibility::ContractError::new(
                "invalid_arguments",
                "Invalid command arguments.",
                "Run the command with --help and correct its arguments.",
                2,
            ))
        }
        Err(error) => error.exit(),
    };
    if let Some(version) = cli.api_version
        && version != compatibility::API_VERSION
    {
        contract_error(compatibility::ContractError::new(
            "unsupported_api_version",
            format!("Integration API {version} is not supported."),
            "Discover supported_api_versions with compatibility --json, then choose a mutually supported version.",
            17,
        ));
    }
    if let Some(cli::Command::Compatibility { scope, store }) = &cli.command {
        match compatibility::run(&cli, *scope, store.as_deref()) {
            Ok(code) => std::process::exit(code),
            Err(error) => emit_contract_error(error),
        }
    }
    if let Some(cli::Command::Prepare {
        scope,
        store,
        registered,
        initialize,
    }) = &cli.command
    {
        match preparation::run(&cli, *scope, store.as_deref(), *registered, *initialize) {
            Ok(code) => std::process::exit(code),
            Err(error) => emit_contract_error(error),
        }
    }
    let json = cli.json;
    let versioned = cli.api_version.is_some();
    if let Err(error) = app::run(cli) {
        if versioned {
            emit_contract_error(error);
        }
        if json {
            eprintln!("{}", serde_json::json!({"error": format!("{error:#}")}));
        } else {
            eprintln!("omni-graph: {error:#}");
        }
        std::process::exit(1);
    }
}
fn emit_contract_error(error: anyhow::Error) -> ! {
    if let Some(database) = error.downcast_ref::<database::DatabaseError>() {
        contract_error(compatibility::ContractError::new(
            database.code,
            database.message.clone(),
            database.recovery,
            if matches!(database.code, "unsupported_schema" | "schema_changed") {
                12
            } else {
                1
            },
        ));
    }
    if let Ok(error) = error.downcast::<compatibility::ContractError>() {
        contract_error(error);
    }
    // Do not echo arbitrary parser/database/input errors: they may contain memory data.
    contract_error(compatibility::ContractError::new(
        "operation_failed",
        "The requested operation failed.",
        "Check input and permissions. Run compatibility --json to diagnose store compatibility. Preserve the store; do not retry a creation blindly.",
        1,
    ));
}
