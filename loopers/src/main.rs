#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]

extern crate bytes;
extern crate chrono;
extern crate crossbeam_queue;
extern crate dirs;
extern crate futures;
extern crate jack;
extern crate serde;
#[macro_use]
extern crate log;

mod loopers_jack;

#[cfg(target_os = "macos")]
mod looper_coreaudio;

use crate::loopers_jack::jack_main;
use clap::{arg, Parser};
use crossbeam_channel::bounded;
use loopers_common::gui_channel::GuiSender;
use loopers_gui::Gui;
use std::io;
use std::process::exit;

// metronome sounds; included in the binary for now to ease usage of cargo install
const SINE_NORMAL: &[u8] = include_bytes!("../resources/sine_normal.wav");
const SINE_EMPHASIS: &[u8] = include_bytes!("../resources/sine_emphasis.wav");

#[cfg(target_os = "macos")]
const DEFAULT_DRIVER: &str = "coreaudio";

#[cfg(not(target_os = "macos"))]
const DEFAULT_DRIVER: &str = "jack";

#[derive(Parser)]
#[command(
    version = "0.1.2",
    about = "Loopers is a graphical live looper, designed for ease of use and rock-solid stability"
)]
struct Cli {
    /// Automatically restores the last saved session
    #[arg(long, default_value_t = false)]
    restore: bool,

    /// Launches in headless mode (without the gui)
    #[arg(long, default_value_t = false)]
    no_gui: bool,

    #[arg(
        long,
        default_value_t = DEFAULT_DRIVER.to_string(),
        help = format!(
            "Controls which audio driver to use (included drivers: {})",
            if cfg!(feature = "coreaudio-rs") {
                "coreaudio, jack"
            } else {
                "jack"
            }
        ),
    )]
    driver: String,

    /// Enable debug logging
    #[arg(long, default_value_t = false)]
    debug: bool,

    /// Path to output logs to
    #[arg(long, default_value_t = String::new())]
    log_path: String,
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = setup_logger(cli.debug, &cli.log_path) {
        eprintln!("Unable to set up logging: {:?}", e);
    }

    if cli.restore {
        info!("Restoring previous session");
    }

    let (gui_to_engine_sender, gui_to_engine_receiver) = bounded(100);

    let (gui, gui_sender) = if !cli.no_gui {
        let (sender, receiver) = GuiSender::new();
        (
            Some(Gui::new(receiver, gui_to_engine_sender, sender.clone())),
            sender,
        )
    } else {
        (None, GuiSender::disconnected())
    };

    // read wav files
    let reader = hound::WavReader::new(SINE_NORMAL).unwrap();
    let beat_normal: Vec<f32> = reader.into_samples().map(|x| x.unwrap()).collect();

    let reader = hound::WavReader::new(SINE_EMPHASIS).unwrap();
    let beat_emphasis: Vec<f32> = reader.into_samples().map(|x| x.unwrap()).collect();

    match cli.driver.as_str() {
        "jack" => {
            jack_main(
                gui,
                gui_sender,
                gui_to_engine_receiver,
                beat_normal,
                beat_emphasis,
                cli.restore,
            );
        }
        "coreaudio" => {
            if cfg!(target_os = "macos") {
                #[cfg(target_os = "macos")]
                crate::looper_coreaudio::coreaudio_main(
                    gui,
                    gui_sender,
                    gui_to_engine_receiver,
                    beat_normal,
                    beat_emphasis,
                    cli.restore,
                )
                .expect("failed to set up coreaudio");
            } else {
                eprintln!("Coreaudio is not supported on this system; choose another driver");
                exit(1);
            }
        }
        driver => {
            eprintln!("Unknown driver '{}'", driver);
            exit(1);
        }
    }
}

fn setup_logger(debug: bool, path: &str) -> Result<(), fern::InitError> {
    let level = if debug {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    let stdout_config = fern::Dispatch::new().chain(io::stdout()).level(level);

    let mut d = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{}[{}][{}] {}",
                chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                record.target(),
                record.level(),
                message
            ))
        })
        .chain(stdout_config);

    if !path.is_empty() {
        let file_config = fern::Dispatch::new()
            .chain(fern::log_file(path)?)
            .level(level);

        d = d.chain(file_config);
    };

    d.apply()?;

    Ok(())
}
