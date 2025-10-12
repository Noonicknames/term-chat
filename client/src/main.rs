use std::process::ExitCode;

use clap::Parser;
use flexi_logger::{FileSpec, Logger};
use log::error;

use crate::app::run_app;

pub mod app;

/// Client for term-chat
#[derive(clap::Parser)]
pub struct CommandArgs {
    name: String,
}

fn main() -> ExitCode {
    Logger::try_with_env_or_str("info") // use RUST_LOG if set, or fallback to "info"
        .unwrap()
        .log_to_file(FileSpec::default().directory("logs").basename("app"))
        .append() // don't overwrite on restart
        .start()
        .unwrap();

    let args = CommandArgs::parse();

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            error!("Error occurred while building tokio runtime: {}", err);
            return ExitCode::FAILURE;
        }
    };

    if let Err(err) = rt.block_on(run_app(args)) {
        error!("Error occurred: {}", err);
        return ExitCode::FAILURE;
    }

    return ExitCode::SUCCESS;
}
