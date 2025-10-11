use std::{num::NonZero, process::ExitCode, sync::Arc};

use clap::Parser;
use log::{error, info, warn};

use crate::server::{Server, ServerSettings};

pub mod error;
pub mod server;

/// Server backend for term-chat
#[derive(clap::Parser)]
pub struct Command {
    /// What socket address to listen to.
    ///
    /// Example: "127.0.0.1:6942"
    listen_address: String,
    #[arg(long, default_value_t = 64)]
    max_concurrency: usize,
    #[arg(long, default_value_t = 1024)]
    max_message_buffer_size: usize,
}

fn main() -> ExitCode {
    env_logger::init();

    let Command {
        listen_address,
        max_concurrency,
        max_message_buffer_size,
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

    let server_settings = ServerSettings {
        max_concurrency,
        max_message_buffer_size,
    };

    let server = match rt.block_on(Server::new(listen_address, server_settings)) {
        Ok(server) => Arc::new(server),
        Err(err) => {
            error!("Error occurred: {}", err);
            return ExitCode::FAILURE;
        }
    };

    if let Err(err) = rt.block_on(server.run_loop()) {
        error!("Error occurred: {}", err);
    }

    return ExitCode::SUCCESS;
}
