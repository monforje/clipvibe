use crate::store::Entry;
use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Ping,
    List,
    Copy { id: u64 },
    CopyText { text: String },
    TogglePin { id: u64 },
    Delete { id: u64 },
    ClearUnpinned,
    Quit,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Entries { entries: Vec<Entry> },
    Error { message: String },
}

fn runtime_dir() -> PathBuf {
    dirs::runtime_dir().unwrap_or_else(std::env::temp_dir)
}

pub fn daemon_socket() -> PathBuf {
    runtime_dir().join("clipvibe.sock")
}

pub fn ui_socket() -> PathBuf {
    runtime_dir().join("clipvibe-ui.sock")
}

pub fn send(req: &Request) -> Result<Response> {
    let mut stream = UnixStream::connect(daemon_socket()).context("daemon is not running")?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    let mut reader = BufReader::new(stream);
    let mut resp = String::new();
    reader.read_line(&mut resp)?;
    Ok(serde_json::from_str(&resp)?)
}

pub fn daemon_alive() -> bool {
    matches!(send(&Request::Ping), Ok(Response::Ok))
}

/// Starts the daemon as a detached process if it isn't running yet.
pub fn ensure_daemon() -> Result<()> {
    ensure_daemon_from(&std::env::current_exe()?)
}

pub fn ensure_daemon_from(exe: &std::path::Path) -> Result<()> {
    if daemon_alive() {
        return Ok(());
    }
    use std::os::unix::process::CommandExt;
    std::process::Command::new(exe)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()?;
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(20));
        if daemon_alive() {
            return Ok(());
        }
    }
    anyhow::bail!("daemon did not start")
}
