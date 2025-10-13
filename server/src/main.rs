use std::{process::ExitCode, sync::Arc};

use clap::Parser;
use log::{error, info};

use crate::server::{Server, ServerSettings};

pub mod error;
pub mod server;

/// Server backend for term-chat
#[derive(clap::Parser)]
pub enum Command {
    /// Create a new settings file to load.
    New {
        /// Path to write settings file.
        #[arg(long, default_value = "server-settings.ron")]
        path: String,
        /// Overwrite if already exists?
        #[arg(long, default_value_t = false)]
        overwrite: bool,
    },
    /// Run normally from a `server-settings.ron` file.
    Run {
        /// Path to settings file.
        #[arg(long, default_value = "server-settings.ron")]
        path: String,
    },
    /// Direct input through command line interface.
    Cli {
        listen_addresses: Vec<String>,
        #[arg(long, default_value_t = 64)]
        max_concurrency: usize,
        #[arg(long, default_value_t = 2048)]
        max_message_buffer_size: usize,
    },
}

fn main() -> ExitCode {
    env_logger::init();

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            error!("Error occurred while building tokio runtime: {}", err);
            return ExitCode::FAILURE;
        }
    };

    match Command::parse() {
        Command::New { path, overwrite } => {
            let settings = ServerSettings::default();
            let settings_ser =
                ron::ser::to_string_pretty(&settings, ron::ser::PrettyConfig::new()).unwrap();
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .create_new(!overwrite)
                .open(path)
                .unwrap();

            write!(&mut file, "{}", settings_ser).unwrap();
        }
        Command::Run { path } => {
            let settings_ser = std::fs::read(path).unwrap();
            let server_settings = ron::de::from_bytes(&settings_ser).unwrap();
            let server = match rt.block_on(Server::new(server_settings)) {
                Ok(server) => Arc::new(server),
                Err(err) => {
                    error!("Error occurred: {}", err);
                    return ExitCode::FAILURE;
                }
            };

            if let Err(err) = rt.block_on(server.run_loop()) {
                error!("Error occurred: {}", err);
            }
        }
        Command::Cli {
            listen_addresses,
            max_concurrency,
            max_message_buffer_size,
        } => {
            let server_settings = ServerSettings {
                listen_addresses: listen_addresses
                    .iter()
                    .map(|addresss| addresss.parse().unwrap())
                    .collect(),
                max_concurrency,
                max_message_buffer_size,
            };

            let server = match rt.block_on(Server::new(server_settings)) {
                Ok(server) => Arc::new(server),
                Err(err) => {
                    error!("Error occurred: {}", err);
                    return ExitCode::FAILURE;
                }
            };

            if let Err(err) = rt.block_on(server.run_loop()) {
                error!("Error occurred: {}", err);
            }
        }
    }

    return ExitCode::SUCCESS;
}
