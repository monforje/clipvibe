//! Inline Lucide-style icons served through gpui's asset system.

use crate::store::Kind;
use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

#[derive(Clone, Copy)]
pub enum Icon {
    Search,
    Text,
    Link,
    Image,
    Code,
    Palette,
    Folder,
    Pin,
    Trash,
    Copy,
    Clipboard,
    Keyboard,
    Layers,
    Check,
}

impl Icon {
    pub fn path(self) -> &'static str {
        match self {
            Icon::Search => "icons/search.svg",
            Icon::Text => "icons/text.svg",
            Icon::Link => "icons/link.svg",
            Icon::Image => "icons/image.svg",
            Icon::Code => "icons/code.svg",
            Icon::Palette => "icons/palette.svg",
            Icon::Folder => "icons/folder.svg",
            Icon::Pin => "icons/pin.svg",
            Icon::Trash => "icons/trash.svg",
            Icon::Copy => "icons/copy.svg",
            Icon::Clipboard => "icons/clipboard.svg",
            Icon::Keyboard => "icons/keyboard.svg",
            Icon::Layers => "icons/layers.svg",
            Icon::Check => "icons/check.svg",
        }
    }

    pub fn for_kind(kind: Kind) -> Self {
        match kind {
            Kind::Text => Icon::Text,
            Kind::Link => Icon::Link,
            Kind::Color => Icon::Palette,
            Kind::Path => Icon::Folder,
            Kind::Code => Icon::Code,
            Kind::Image => Icon::Image,
        }
    }
}

fn body(path: &str) -> Option<&'static str> {
    Some(match path {
        "icons/search.svg" => r#"<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>"#,
        "icons/text.svg" => r#"<path d="M4 6h16M4 12h16M4 18h10"/>"#,
        "icons/link.svg" => {
            r#"<path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/>"#
        }
        "icons/image.svg" => {
            r#"<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.1-3.1a2 2 0 0 0-2.8 0L6 21"/>"#
        }
        "icons/code.svg" => r#"<path d="m16 18 6-6-6-6M8 6l-6 6 6 6"/>"#,
        "icons/palette.svg" => {
            r#"<circle cx="13.5" cy="6.5" r="1"/><circle cx="17.5" cy="10.5" r="1"/><circle cx="8.5" cy="7.5" r="1"/><circle cx="6.5" cy="12.5" r="1"/><path d="M12 2a10 10 0 0 0 0 20c.93 0 1.65-.75 1.65-1.69 0-.44-.18-.84-.44-1.13-.29-.29-.44-.65-.44-1.13A1.64 1.64 0 0 1 14.44 16.4h2A5.56 5.56 0 0 0 22 10.84C22 6 17.5 2 12 2z"/>"#
        }
        "icons/folder.svg" => {
            r#"<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>"#
        }
        "icons/pin.svg" => {
            r#"<path d="M12 17v5"/><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"/>"#
        }
        "icons/trash.svg" => {
            r#"<path d="M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M10 11v6M14 11v6"/>"#
        }
        "icons/copy.svg" => {
            r#"<rect x="8" y="8" width="14" height="14" rx="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>"#
        }
        "icons/clipboard.svg" => {
            r#"<rect x="8" y="2" width="8" height="4" rx="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/>"#
        }
        "icons/keyboard.svg" => {
            r#"<rect x="2" y="4" width="20" height="16" rx="2"/><path d="M6 8h.01M10 8h.01M14 8h.01M18 8h.01M6 12h.01M10 12h.01M14 12h.01M18 12h.01M7 16h10"/>"#
        }
        "icons/layers.svg" => {
            r#"<path d="m12.83 2.18 8.58 3.9a1 1 0 0 1 0 1.83l-8.58 3.9a2 2 0 0 1-1.66 0L2.6 7.91a1 1 0 0 1 0-1.83l8.58-3.9a2 2 0 0 1 1.66 0Z"/><path d="m22 12-9.17 4.17a2 2 0 0 1-1.66 0L2 12"/><path d="m22 17-9.17 4.17a2 2 0 0 1-1.66 0L2 17"/>"#
        }
        "icons/check.svg" => r#"<path d="M20 6 9 17l-5-5"/>"#,
        _ => return None,
    })
}

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(body(path).map(|b| {
            Cow::Owned(
                format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{b}</svg>"#
                )
                .into_bytes(),
            )
        }))
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
