use std::{num::NonZero, process::ExitCode, sync::Arc};

use clap::Parser;
use log::{error, info, warn};

use crate::client::Client;

pub mod client;

/// client backend for term-chat
#[derive(clap::Parser)]
pub struct Command {
    /// What socket address to listen to.
    ///
    /// Example: "127.0.0.1:6942"
    #[arg(long, default_value = "127.0.0.1:6942")]
    client_address: String,
}

fn main() -> ExitCode {
    env_logger::init();

    let Command {
        client_address,
    } = Command::parse();

    let available_parallelism = match std::thread::available_parallelism() {
        Ok(available_parallelism) => available_parallelism,
        Err(err) => {
            warn!(
                "Error occurred fetching available parallelism: {}\nWill be using single thread.",
                err
            );
            unsafe { NonZero::new_unchecked(1) }
        }
    };

    info!("Available parallelism: {}", available_parallelism);

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(available_parallelism.get())
        .enable_io()
        .max_blocking_threads(available_parallelism.get())
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            error!("Error occurred while building tokio runtime: {}", err);
            return ExitCode::FAILURE;
        }
    };

    let client = match rt.block_on(Client::new()) {
        Ok(client) => Arc::new(client),
        Err(err) => {
            error!("Error occurred: {}", err);
            return ExitCode::FAILURE;
        }
    };

    if let Err(err) = rt.block_on(client.run()) {
        error!("Error occurred: {}", err);
    }

    return ExitCode::SUCCESS;
}
