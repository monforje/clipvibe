use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_UNPINNED: usize = 1000;
pub const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Entry {
    pub id: u64,
    pub hash: u64,
    #[serde(flatten)]
    pub content: Content,
    #[serde(default)]
    pub pinned: bool,
    pub created: i64,
    pub last_used: i64,
    #[serde(default)]
    pub uses: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Content {
    Text {
        text: String,
    },
    Image {
        file: String,
        width: u32,
        height: u32,
    },
}

/// What the UI shows: derived from content, never stored.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Text,
    Link,
    Color,
    Path,
    Code,
    Image,
}

impl Entry {
    pub fn text(&self) -> Option<&str> {
        match &self.content {
            Content::Text { text } => Some(text),
            Content::Image { .. } => None,
        }
    }

    pub fn kind(&self) -> Kind {
        match &self.content {
            Content::Image { .. } => Kind::Image,
            Content::Text { text } => classify(text),
        }
    }
}

pub fn classify(text: &str) -> Kind {
    let t = text.trim();
    if t.is_empty() {
        return Kind::Text;
    }
    let single_line = !t.contains('\n');
    if single_line && parse_color(t).is_some() {
        return Kind::Color;
    }
    if single_line && !t.contains(char::is_whitespace) {
        let lower = t.to_ascii_lowercase();
        if [
            "http://", "https://", "ftp://", "mailto:", "magnet:", "ssh://", "git@",
        ]
        .iter()
        .any(|p| lower.starts_with(p))
        {
            return Kind::Link;
        }
        if t.starts_with('/') || t.starts_with("~/") || t.starts_with("file://") {
            return Kind::Path;
        }
    }
    if looks_like_code(t) {
        return Kind::Code;
    }
    Kind::Text
}

fn looks_like_code(t: &str) -> bool {
    let lines: Vec<&str> = t.lines().collect();
    let markers = [
        "fn ",
        "let ",
        "const ",
        "def ",
        "class ",
        "import ",
        "#include",
        "function ",
        "=>",
        "->",
        "::",
        "};",
        "){",
        ") {",
        "</",
        "/>",
        "$(",
        "&&",
        "||",
        "==",
        "!=",
        "pub ",
        "return ",
        "#!/",
    ];
    let hits = lines
        .iter()
        .filter(|l| {
            let l = l.trim();
            l.ends_with(';')
                || l.ends_with('{')
                || l == "}"
                || markers.iter().any(|m| l.contains(m))
        })
        .count();
    if lines.len() >= 2 {
        let indented = lines
            .iter()
            .filter(|l| l.starts_with("    ") || l.starts_with('\t'))
            .count();
        hits * 3 >= lines.len() || (indented >= 2 && hits >= 1)
    } else {
        const SHELL: [&str; 18] = [
            "sudo ",
            "cargo ",
            "git ",
            "npm ",
            "pnpm ",
            "docker ",
            "cd ",
            "ls ",
            "apt ",
            "pip ",
            "python ",
            "curl ",
            "wget ",
            "ssh ",
            "systemctl ",
            "kubectl ",
            "make ",
            "./",
        ];
        if t.len() < 300 && SHELL.iter().any(|p| t.starts_with(p)) {
            return true;
        }
        hits > 0
            && t.len() < 300
            && (t.contains("=>") || t.contains("::") || t.ends_with(';') || t.contains("$("))
    }
}

/// `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb(..)` / `rgba(..)`. Returns 0xRRGGBBAA.
pub fn parse_color(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let v = u32::from_str_radix(hex, 16).ok()?;
        return match hex.len() {
            3 => {
                let (r, g, b) = ((v >> 8) & 0xf, (v >> 4) & 0xf, v & 0xf);
                Some((r * 17) << 24 | (g * 17) << 16 | (b * 17) << 8 | 0xff)
            }
            6 => Some(v << 8 | 0xff),
            8 => Some(v),
            _ => None,
        };
    }
    let lower = s.to_ascii_lowercase();
    let inner = lower
        .strip_prefix("rgba(")
        .or_else(|| lower.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<&str> = inner
        .split([',', ' ', '/'])
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() < 3 || parts.len() > 4 {
        return None;
    }
    let mut out = 0u32;
    for p in &parts[..3] {
        let v: u32 = p.parse().ok()?;
        if v > 255 {
            return None;
        }
        out = out << 8 | v;
    }
    let a = match parts.get(3) {
        Some(a) => {
            let f: f32 = a.parse().ok()?;
            (if f <= 1.0 { f * 255.0 } else { f }).clamp(0.0, 255.0) as u32
        }
        None => 255,
    };
    Some(out << 8 | a)
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn hash_bytes(kind: u8, bytes: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    kind.hash(&mut h);
    bytes.hash(&mut h);
    h.finish()
}

pub fn data_dir() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("clipvibe");
    let _ = std::fs::create_dir_all(dir.join("images"));
    dir
}

pub fn image_path(file: &str) -> PathBuf {
    data_dir().join("images").join(file)
}

#[derive(Serialize, Deserialize, Default)]
pub struct History {
    pub next_id: u64,
    /// Most recently used first.
    pub entries: Vec<Entry>,
}

impl History {
    fn file() -> PathBuf {
        data_dir().join("history.json")
    }

    pub fn load() -> Self {
        std::fs::read(Self::file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::file();
        let tmp = path.with_extension("json.tmp");
        if let Ok(bytes) = serde_json::to_vec(self)
            && std::fs::write(&tmp, bytes).is_ok()
        {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    /// Inserts new content or bumps an existing identical entry to the top.
    /// Returns true if something changed.
    pub fn record(&mut self, hash: u64, content: impl FnOnce() -> Option<Content>) -> bool {
        let now = now();
        if let Some(pos) = self.entries.iter().position(|e| e.hash == hash) {
            if pos == 0 {
                return false;
            }
            let mut e = self.entries.remove(pos);
            e.last_used = now;
            self.entries.insert(0, e);
            return true;
        }
        let Some(content) = content() else {
            return false;
        };
        self.next_id += 1;
        self.entries.insert(
            0,
            Entry {
                id: self.next_id,
                hash,
                content,
                pinned: false,
                created: now,
                last_used: now,
                uses: 0,
            },
        );
        self.prune();
        true
    }

    pub fn touch(&mut self, id: u64) -> Option<Entry> {
        let pos = self.entries.iter().position(|e| e.id == id)?;
        let mut e = self.entries.remove(pos);
        e.last_used = now();
        e.uses += 1;
        self.entries.insert(0, e.clone());
        Some(e)
    }

    pub fn remove(&mut self, id: u64) {
        if let Some(pos) = self.entries.iter().position(|e| e.id == id) {
            let e = self.entries.remove(pos);
            Self::drop_files(&e, &self.entries);
        }
    }

    pub fn clear_unpinned(&mut self) {
        let (keep, gone): (Vec<_>, Vec<_>) = self.entries.drain(..).partition(|e| e.pinned);
        self.entries = keep;
        for e in gone {
            Self::drop_files(&e, &self.entries);
        }
    }

    fn prune(&mut self) {
        let mut unpinned = 0;
        let mut i = 0;
        while i < self.entries.len() {
            if !self.entries[i].pinned {
                unpinned += 1;
                if unpinned > MAX_UNPINNED {
                    let e = self.entries.remove(i);
                    Self::drop_files(&e, &self.entries);
                    continue;
                }
            }
            i += 1;
        }
    }

    fn drop_files(e: &Entry, remaining: &[Entry]) {
        if let Content::Image { file, .. } = &e.content {
            let still_used = remaining
                .iter()
                .any(|o| matches!(&o.content, Content::Image { file: f, .. } if f == file));
            if !still_used {
                let _ = std::fs::remove_file(image_path(file));
            }
        }
    }
}
