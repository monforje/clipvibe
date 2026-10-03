mod fuzzy;
mod icons;
mod theme;
mod view;

use crate::ipc;
use anyhow::Result;
use gpui::{
    App, AppContext as _, Application, Bounds, TitlebarOptions, WindowBackgroundAppearance,
    WindowBounds, WindowDecorations, WindowKind, WindowOptions, px, size,
};
use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};

pub fn run() -> Result<()> {
    // Toggle: a second invocation closes the already open window.
    let sock = ipc::ui_socket();
    if let Ok(mut stream) = UnixStream::connect(&sock) {
        let _ = stream.write_all(b"close\n");
        return Ok(());
    }
    let _ = std::fs::remove_file(&sock);
    let listener = UnixListener::bind(&sock)?;
    std::thread::spawn(move || {
        if listener.accept().is_ok() {
            let _ = std::fs::remove_file(ipc::ui_socket());
            std::process::exit(0);
        }
    });

    ipc::ensure_daemon().ok();

    Application::new()
        .with_assets(icons::Assets)
        .run(|cx: &mut App| {
            let bounds = Bounds::centered(None, size(px(880.), px(600.)), cx);
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        titlebar: Some(TitlebarOptions {
                            title: Some("Clipvibe".into()),
                            appears_transparent: true,
                            traffic_light_position: None,
                        }),
                        focus: true,
                        show: true,
                        kind: WindowKind::Normal,
                        is_resizable: false,
                        is_minimizable: false,
                        window_background: WindowBackgroundAppearance::Transparent,
                        window_decorations: Some(WindowDecorations::Client),
                        app_id: Some("clipvibe".into()),
                        ..Default::default()
                    },
                    |window, cx| cx.new(|cx| view::ClipView::new(window, cx)),
                )
                .expect("failed to open window");
            window
                .update(cx, |view, window, cx| {
                    window.focus(view.focus_handle());
                    cx.activate(true);
                })
                .ok();
            #[cfg(debug_assertions)]
            if let Ok(script) = std::env::var("CLIPVIBE_DEMO") {
                demo(window, script, cx);
            }
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
        });
    let _ = std::fs::remove_file(ipc::ui_socket());
    Ok(())
}

/// Debug helper: replays `;`-separated keystrokes (`text:абв` types text),
/// one step every 1.5s, so the UI can be exercised without real input.
#[cfg(debug_assertions)]
fn demo(window: gpui::WindowHandle<view::ClipView>, script: String, cx: &mut App) {
    cx.spawn(async move |cx| {
        for step in script.split(';') {
            let ms = std::env::var("CLIPVIBE_DEMO_MS")
                .ok()
                .and_then(|v| v.parse().ok());
            gpui::Timer::after(std::time::Duration::from_millis(ms.unwrap_or(1500))).await;
            // Mouse: `mdown:x,y` / `mdown2:x,y` (double click) / `mmove:x,y` / `mup:x,y`.
            if let Some((kind, xy)) = step.split_once(':').filter(|(k, _)| k.starts_with('m'))
                && let Some((x, y)) = xy.split_once(',')
            {
                use gpui::{MouseButton, point, px};
                let position = point(px(x.parse().unwrap_or(0.)), px(y.parse().unwrap_or(0.)));
                let _ = window.update(cx, |view, window, cx| match kind {
                    "mdown" | "mdown2" => view.on_text_mouse_down(
                        &gpui::MouseDownEvent {
                            button: MouseButton::Left,
                            position,
                            modifiers: Default::default(),
                            click_count: if kind == "mdown2" { 2 } else { 1 },
                            first_mouse: false,
                        },
                        window,
                        cx,
                    ),
                    "mup" => view.on_mouse_up(
                        &gpui::MouseUpEvent {
                            button: MouseButton::Left,
                            position,
                            modifiers: Default::default(),
                            click_count: 1,
                        },
                        window,
                        cx,
                    ),
                    _ => view.on_mouse_move(
                        &gpui::MouseMoveEvent {
                            position,
                            pressed_button: Some(MouseButton::Left),
                            modifiers: Default::default(),
                        },
                        window,
                        cx,
                    ),
                });
                continue;
            }
            let keys: Vec<gpui::Keystroke> = match step.strip_prefix("text:") {
                Some(text) => text
                    .chars()
                    .map(|c| gpui::Keystroke {
                        modifiers: Default::default(),
                        key: c.to_string(),
                        key_char: Some(c.to_string()),
                    })
                    .collect(),
                None => gpui::Keystroke::parse(step).into_iter().collect(),
            };
            let _ = cx.update_window(window.into(), |_, window, cx| {
                for k in keys {
                    window.dispatch_keystroke(k, cx);
                }
            });
        }
    })
    .detach();
}
