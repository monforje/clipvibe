//! Background process: watches the clipboard and owns the history.
//!
//! GNOME on Wayland doesn't implement the data-control protocol, so the
//! clipboard is observed through XWayland: Mutter mirrors the Wayland
//! selection into the X11 CLIPBOARD selection, and XFixes tells us whenever
//! its owner changes. Setting the clipboard also goes through X11 – the daemon
//! keeps serving it after the popup window is gone.

use crate::ipc::{self, Request, Response};
use crate::store::{self, Content, History, MAX_TEXT_BYTES};
use anyhow::{Context as _, Result};
use arboard::Clipboard;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::ConnectionExt as _;

type Shared = Arc<Mutex<History>>;

pub fn run() -> Result<()> {
    if ipc::daemon_alive() {
        eprintln!("clipvibe daemon is already running");
        return Ok(());
    }
    // One writer per history file, even if sockets live in different dirs.
    let lock = std::fs::File::create(store::data_dir().join("daemon.lock"))?;
    // SAFETY: flock on a valid, open file descriptor.
    if unsafe {
        libc::flock(
            std::os::fd::AsRawFd::as_raw_fd(&lock),
            libc::LOCK_EX | libc::LOCK_NB,
        )
    } != 0
    {
        eprintln!("another clipvibe daemon owns {:?}", store::data_dir());
        return Ok(());
    }
    let sock = ipc::daemon_socket();
    let _ = std::fs::remove_file(&sock);
    let listener = UnixListener::bind(&sock).with_context(|| format!("bind {sock:?}"))?;

    let history: Shared = Arc::new(Mutex::new(History::load()));

    {
        let history = history.clone();
        std::thread::Builder::new()
            .name("clipboard-watch".into())
            .spawn(move || {
                loop {
                    if let Err(err) = watch(&history) {
                        eprintln!("clipboard watcher failed: {err:#}; retrying in 2s");
                    }
                    std::thread::sleep(Duration::from_secs(2));
                }
            })?;
    }

    let mut clipboard = Clipboard::new().context("open X11 clipboard")?;
    let _lock = lock;
    eprintln!("clipvibe daemon listening on {sock:?}");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(err) = serve(stream, &history, &mut clipboard) {
                    eprintln!("client error: {err:#}");
                }
            }
            Err(err) => eprintln!("accept failed: {err}"),
        }
    }
    Ok(())
}

fn serve(stream: UnixStream, history: &Shared, clipboard: &mut Clipboard) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let req: Request = serde_json::from_str(&line)?;
    if matches!(req, Request::Quit) {
        (&stream).write_all(b"{\"status\":\"ok\"}\n")?;
        let _ = std::fs::remove_file(ipc::daemon_socket());
        std::process::exit(0);
    }
    let resp = handle(req, history, clipboard).unwrap_or_else(|err| Response::Error {
        message: format!("{err:#}"),
    });
    let mut out = serde_json::to_string(&resp)?;
    out.push('\n');
    (&stream).write_all(out.as_bytes())?;
    Ok(())
}

fn handle(req: Request, history: &Shared, clipboard: &mut Clipboard) -> Result<Response> {
    let mut h = history.lock().unwrap();
    match req {
        Request::Ping => return Ok(Response::Ok),
        Request::List => {
            return Ok(Response::Entries {
                entries: h.entries.clone(),
            });
        }
        Request::Copy { id } => {
            let entry = h.touch(id).context("no such entry")?;
            h.save();
            // Release the lock before talking to X: the watcher will read our
            // own selection back and needs the history too.
            drop(h);
            match entry.content {
                Content::Text { text } => clipboard.set_text(text)?,
                Content::Image { file, .. } => {
                    let img = image::open(store::image_path(&file))?.into_rgba8();
                    clipboard.set_image(arboard::ImageData {
                        width: img.width() as usize,
                        height: img.height() as usize,
                        bytes: img.into_raw().into(),
                    })?;
                }
            }
            return Ok(Response::Ok);
        }
        Request::CopyText { text } => {
            drop(h);
            clipboard.set_text(text)?;
            return Ok(Response::Ok);
        }
        Request::TogglePin { id } => {
            if let Some(e) = h.entries.iter_mut().find(|e| e.id == id) {
                e.pinned = !e.pinned;
            }
        }
        Request::Delete { id } => h.remove(id),
        Request::ClearUnpinned => h.clear_unpinned(),
        Request::Quit => unreachable!("handled in serve"),
    }
    h.save();
    Ok(Response::Ok)
}

fn watch(history: &Shared) -> Result<()> {
    let (conn, screen) = x11rb::connect(None).context("connect to X11/XWayland")?;
    let root = conn.setup().roots[screen].root;
    conn.xfixes_query_version(5, 0)?.reply()?;
    let selection = conn.intern_atom(false, b"CLIPBOARD")?.reply()?.atom;
    conn.xfixes_select_selection_input(
        root,
        selection,
        SelectionEventMask::SET_SELECTION_OWNER
            | SelectionEventMask::SELECTION_WINDOW_DESTROY
            | SelectionEventMask::SELECTION_CLIENT_CLOSE,
    )?;
    conn.flush()?;

    let mut clipboard = Clipboard::new()?;
    capture(&mut clipboard, history);
    loop {
        let event = conn.wait_for_event()?;
        if !matches!(event, Event::XfixesSelectionNotify(_)) {
            continue;
        }
        // Apps often re-announce the selection several times in a row.
        std::thread::sleep(Duration::from_millis(60));
        while conn.poll_for_event()?.is_some() {}
        capture(&mut clipboard, history);
    }
}

fn capture(clipboard: &mut Clipboard, history: &Shared) {
    if let Ok(text) = clipboard.get_text() {
        if text.trim().is_empty() || text.len() > MAX_TEXT_BYTES {
            return;
        }
        let hash = store::hash_bytes(0, text.as_bytes());
        let mut h = history.lock().unwrap();
        if h.record(hash, || Some(Content::Text { text })) {
            h.save();
        }
        return;
    }
    if let Ok(img) = clipboard.get_image() {
        let hash = store::hash_bytes(1, &img.bytes);
        let mut h = history.lock().unwrap();
        let changed = h.record(hash, || {
            let file = format!("{hash:016x}.png");
            let buf = image::RgbaImage::from_raw(
                img.width as u32,
                img.height as u32,
                img.bytes.into_owned(),
            )?;
            buf.save(store::image_path(&file)).ok()?;
            Some(Content::Image {
                file,
                width: img.width as u32,
                height: img.height as u32,
            })
        });
        if changed {
            h.save();
        }
    }
}
