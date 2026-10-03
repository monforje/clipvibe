mod daemon;
mod ipc;
mod store;
mod ui;

use anyhow::Result;

const HELP: &str = "\
clipvibe — история буфера обмена

  clipvibe            открыть/закрыть окно истории (повесьте на хоткей)
  clipvibe daemon     запустить фоновый сборщик (стартует сам при первом открытии)
  clipvibe list       вывести историю в терминал
  clipvibe clear      удалить всё, кроме закреплённого
  clipvibe install    автозапуск + ярлык + хоткей GNOME (Super+Shift+V)
";

fn main() -> Result<()> {
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        None | Some("show" | "toggle") => ui::run(),
        Some("daemon") => daemon::run(),
        Some("list") => {
            ipc::ensure_daemon()?;
            use std::io::Write;
            let mut out = std::io::stdout().lock();
            if let ipc::Response::Entries { entries } = ipc::send(&ipc::Request::List)? {
                for e in entries {
                    let pin = if e.pinned { "★" } else { " " };
                    let body = match e.text() {
                        Some(t) => t.lines().next().unwrap_or("").chars().take(100).collect(),
                        None => "[изображение]".to_string(),
                    };
                    if writeln!(out, "{pin} {:>5}  {:?}  {body}", e.id, e.kind()).is_err() {
                        break;
                    }
                }
            }
            Ok(())
        }
        Some("clear") => {
            ipc::ensure_daemon()?;
            ipc::send(&ipc::Request::ClearUnpinned)?;
            Ok(())
        }
        Some("install") => install(),
        _ => {
            print!("{HELP}");
            Ok(())
        }
    }
}

fn install() -> Result<()> {
    let home = dirs::home_dir().expect("no home dir");
    let bin_dir = home.join(".local/bin");
    std::fs::create_dir_all(&bin_dir)?;
    let bin = bin_dir.join("clipvibe");
    let exe = std::env::current_exe()?;
    if exe != bin {
        let tmp = bin.with_extension("new");
        std::fs::copy(&exe, &tmp)?;
        std::fs::rename(&tmp, &bin)?;
    }
    let bin_path = bin.clone();
    let bin = bin.display();

    let desktop = |name: &str, exec: &str, extra: &str| {
        format!(
            "[Desktop Entry]\nType=Application\nName={name}\nComment=История буфера обмена\n\
             Exec={exec}\nIcon=edit-paste\nTerminal=false\nCategories=Utility;\n{extra}"
        )
    };
    let apps = home.join(".local/share/applications");
    std::fs::create_dir_all(&apps)?;
    std::fs::write(
        apps.join("clipvibe.desktop"),
        desktop("Clipvibe", &bin.to_string(), "StartupWMClass=clipvibe\n"),
    )?;
    let autostart = home.join(".config/autostart");
    std::fs::create_dir_all(&autostart)?;
    std::fs::write(
        autostart.join("clipvibe-daemon.desktop"),
        desktop(
            "Clipvibe daemon",
            &format!("{bin} daemon"),
            "NoDisplay=true\nX-GNOME-Autostart-enabled=true\n",
        ),
    )?;
    println!("✓ бинарник: {bin}");
    println!("✓ автозапуск демона: ~/.config/autostart/clipvibe-daemon.desktop");

    // GNOME custom keybinding.
    let base = "org.gnome.settings-daemon.plugins.media-keys";
    let path = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/clipvibe/";
    let gs = |args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("gsettings")
            .args(args)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    if let Some(list) = gs(&["get", base, "custom-keybindings"]) {
        if !list.contains(path) {
            let new_list = if list.contains("[]") {
                format!("['{path}']")
            } else {
                list.trim_end_matches(']').to_string() + &format!(", '{path}']")
            };
            gs(&["set", base, "custom-keybindings", &new_list]);
        }
        let schema = format!("{base}.custom-keybinding:{path}");
        gs(&["set", &schema, "name", "Clipvibe"]);
        gs(&["set", &schema, "command", &bin.to_string()]);
        gs(&["set", &schema, "binding", "<Super><Shift>v"]);
        println!("✓ хоткей GNOME: Super+Shift+V");
    }
    // Restart the daemon from the installed binary.
    if ipc::daemon_alive() {
        let _ = ipc::send(&ipc::Request::Quit);
    }
    ipc::ensure_daemon_from(&bin_path)?;
    println!("✓ демон запущен");
    Ok(())
}
