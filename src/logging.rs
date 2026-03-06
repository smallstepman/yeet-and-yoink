use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use tracing_subscriber::EnvFilter;

fn debug_env_enabled() -> bool {
    let value = match std::env::var("NIRI_DEEP_DEBUG") {
        Ok(value) => value,
        Err(_) => return false,
    };

    let value = value.trim().to_ascii_lowercase();
    !(value.is_empty() || value == "0" || value == "false" || value == "off" || value == "no")
}

fn open_log_file(path: &Path, append: bool) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).write(true);
    if append {
        options.append(true);
    } else {
        options.truncate(true);
    }
    options.open(path)
}

pub fn init(log_file: Option<&Path>, append: bool) {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let default_filter = if debug_env_enabled() || log_file.is_some() {
            "debug"
        } else {
            "off"
        };
        let env_filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));

        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(false)
            .without_time()
            .compact();

        let init_result = match log_file {
            Some(path) => match open_log_file(path, append) {
                Ok(file) => subscriber.with_writer(Mutex::new(file)).try_init(),
                Err(err) => {
                    eprintln!(
                        "yeet-and-yoink: failed to open log file {}: {err}",
                        path.display()
                    );
                    subscriber.try_init()
                }
            },
            None => subscriber.try_init(),
        };

        if let Err(err) = init_result {
            eprintln!("yeet-and-yoink: failed to initialize logging: {err}");
        }
    });
}

pub fn debug(message: impl std::fmt::Display) {
    tracing::debug!("{message}");
}
