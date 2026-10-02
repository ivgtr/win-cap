#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod encoder;
#[cfg(windows)]
mod recorder;
#[cfg(windows)]
mod ui;

#[cfg(windows)]
fn main() {
    if let Err(error) = ui::run() {
        ui::show_error(None, &error);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("win-cap は Windows 10 2004 以降 / Windows 11 専用です。");
    std::process::exit(1);
}
