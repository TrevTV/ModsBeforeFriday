mod downloads;
mod handlers;
mod manifest;
mod mod_man;
mod models;
mod patching;
mod parameters;

use anyhow::{anyhow, Context, Result};
use downloads::DownloadConfig;
use log::{error, Level};
use mbf_res_man::res_cache::ResCache;
use models::{request, response};
use parameters::{init_parameters, PARAMETERS};
use std::{
    io::{BufRead, BufReader, Write},
    panic,
    process::Command,
    sync,
};

#[cfg(feature = "request_timing")]
use log::info;
#[cfg(feature = "request_timing")]
use std::time::Instant;

static DOWNLOAD_CFG: sync::OnceLock<DownloadConfig> = sync::OnceLock::new();

/// Gets the default config used for downloads in MBF
pub fn get_dl_cfg() -> &'static DownloadConfig<'static> {
    DOWNLOAD_CFG.get_or_init(|| {
        DownloadConfig {
            max_disconnections: 10,
            // If downloads data successfully for 10 seconds, reset disconnection attempts
            disconnection_reset_time: Some(std::time::Duration::from_secs_f32(10.0)),
            disconnect_wait_time: std::time::Duration::from_secs_f32(5.0),
            progress_update_interval: Some(std::time::Duration::from_secs_f32(2.0)),
            ureq_agent: mbf_res_man::default_agent::get_agent(),
        }
    })
}

/// Creates a ResCache for downloading files using mbf_res_man
/// This should be reused where possible.
pub fn load_res_cache() -> Result<ResCache<'static>> {
    std::fs::create_dir_all(&PARAMETERS.res_cache).expect("Failed to create resource cache folder");
    Ok(ResCache::new(
        (&PARAMETERS.res_cache).into(),
        mbf_res_man::default_agent::get_agent(),
    ))
}

pub fn get_apk_path() -> Result<Option<String>> {
    let pm_output = Command::new("pm")
        .args(["path", &PARAMETERS.apk_id])
        .output()
        .context("Working out APK path")?;

    let mut paths: Vec<String> = std::str::from_utf8(&pm_output.stdout)?
        .lines()
        .filter_map(|line| line.strip_prefix("package:"))
        .map(|path| path.trim_end().to_owned())
        .collect();

    if paths.is_empty() {
        // App not installed
        return Ok(None);
    }

    // MBF is only able to patch a single (base) APK. Store apps on Quest are
    // distributed as a single APK (plus OBBs), so split APKs indicate an unsupported install.
    if paths.len() > 1 {
        return Err(anyhow!(
            "{} appears to be installed as {} split APK files. Split installs are not supported.",
            &PARAMETERS.apk_id,
            paths.len()
        ));
    }

    Ok(Some(paths.remove(0)))
}

struct ResponseLogger {}

impl log::Log for ResponseLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= Level::Debug
    }

    fn log(&self, record: &log::Record) {
        // Skip logs that are not from mbf_agent, mbf_zip, etc.
        // ...as these are spammy logs from ureq or rustls, and we do nto want them.
        match record.module_path() {
            Some(module_path) => {
                if !module_path.starts_with("mbf") {
                    return;
                }
            }
            None => return,
        }

        // Ignore errors, logging should be infallible and we don't want to panic
        let _result = write_response(response::Response::LogMsg {
            message: format!("{}", record.args()),
            level: match record.level() {
                Level::Debug => response::LogLevel::Debug,
                Level::Info => response::LogLevel::Info,
                Level::Warn => response::LogLevel::Warn,
                Level::Error => response::LogLevel::Error,
                Level::Trace => response::LogLevel::Trace,
            },
        });
    }

    fn flush(&self) {
        let _ = std::io::stdout().flush();
    }
}

fn write_response(response: response::Response) -> Result<()> {
    let mut lock = std::io::stdout().lock();
    serde_json::to_writer(&mut lock, &response).context("Serializing JSON response")?;
    writeln!(lock)?;
    Ok(())
}

static LOGGER: ResponseLogger = ResponseLogger {};

fn main() -> Result<()> {
    #[cfg(feature = "request_timing")]
    let start_time = Instant::now();

    log::set_logger(&LOGGER).expect("Failed to set up logging");
    log::set_max_level(log::LevelFilter::Debug);

    let mut reader = BufReader::new(std::io::stdin());
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let req: request::Request = serde_json::from_str(&line)?;

    // Set the parameters for this instance of the agent
    init_parameters(&req.agent_parameters.game_id, req.agent_parameters.ignore_package_id);

    // Set a panic hook that writes the panic as a JSON Log
    // (we don't do this in catch_unwind as we get an `Any` there, which doesn't implement Display)
    panic::set_hook(Box::new(|info| {
        error!("Request failed due to a panic!: {info}")
    }));

    match std::panic::catch_unwind(|| handlers::handle_request(req)) {
        Ok(resp) => match resp {
            Ok(resp) => {
                #[cfg(feature = "request_timing")]
                {
                    let req_time = Instant::now() - start_time;
                    info!("Request complete in {}ms", req_time.as_millis());
                }

                write_response(resp)?;
            }
            Err(err) => error!("{err:?}"),
        },
        Err(_) => {} // Panic will be outputted above
    };

    Ok(())
}
