mod app;
mod cli;
mod database;
mod install;
mod integrations;
mod model;
mod retrieval;
mod storage;
mod transfer;
mod tui;
mod ui;
mod update;
mod web;

use clap::Parser;
fn main() {
    let cli = cli::Cli::parse();
    let json = cli.json;
    if let Err(error) = app::run(cli) {
        if json {
            eprintln!("{}", serde_json::json!({"error": format!("{error:#}")}));
        } else {
            eprintln!("omni-graph: {error:#}");
        }
        std::process::exit(1);
    }
}
