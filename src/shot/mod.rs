//! Screenshots and screen recordings (`clipvibe shot [--video] [--delay MS]`).

pub mod capture;
mod draw;
mod overlay;
pub mod record;

use crate::ipc;
use anyhow::Result;
use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::Duration;

pub fn run(args: &[String]) -> Result<()> {
    // The same hotkey stops a running recording…
    if record::stop_if_running() {
        return Ok(());
    }
    // …and closes an open overlay.
    let sock = ipc::ui_socket().with_file_name("clipvibe-shot.sock");
    if let Ok(mut stream) = UnixStream::connect(&sock) {
        let _ = stream.write_all(b"close\n");
        return Ok(());
    }
    let _ = std::fs::remove_file(&sock);
    let listener = UnixListener::bind(&sock)?;
    {
        let sock = sock.clone();
        std::thread::spawn(move || {
            if listener.accept().is_ok() {
                let _ = std::fs::remove_file(sock);
                std::process::exit(0);
            }
        });
    }

    let video = args.iter().any(|a| a == "--video");
    let delay = args
        .iter()
        .position(|a| a == "--delay")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    std::thread::sleep(Duration::from_millis(delay));

    let result = capture_and_show(video);
    let _ = std::fs::remove_file(&sock);
    if let Err(err) = &result
        && let Ok(n) = capture::Notifier::new()
    {
        let _ = n.notify(
            "dialog-error",
            "Скриншот не удался",
            &format!("{err:#}"),
            &[],
            false,
        );
    }
    result
}

fn capture_and_show(video: bool) -> Result<()> {
    let shot = capture::screenshot()?;
    let dir = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("clipvibe");
    std::fs::create_dir_all(&dir)?;
    let screen = dir.join("screen.png");
    if std::fs::rename(&shot, &screen).is_err() {
        std::fs::copy(&shot, &screen)?;
        let _ = std::fs::remove_file(&shot);
    }
    overlay::run(screen, video)
}
