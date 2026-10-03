use crate::store::Kind;
use gpui::{Hsla, WindowAppearance, rgb, rgba};

#[derive(Clone, Copy)]
pub struct Theme {
    pub surface: Hsla,
    pub panel: Hsla,
    pub elevated: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_faint: Hsla,
    pub hover: Hsla,
    pub selected: Hsla,
    pub accent: Hsla,
    pub accent_soft: Hsla,
    pub match_fg: Hsla,
    pub match_bg: Hsla,
    pub text_selection: Hsla,
    pub danger: Hsla,
    pub shadow: Hsla,
    pub kbd_bg: Hsla,
    pub link: Hsla,
    pub code: Hsla,
    pub color: Hsla,
    pub path: Hsla,
    pub image: Hsla,
}

impl Theme {
    /// Follows the system, unless `CLIPVIBE_THEME=dark|light` is set.
    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match std::env::var("CLIPVIBE_THEME").as_deref() {
            Ok("dark") => return Self::dark(),
            Ok("light") => return Self::light(),
            _ => {}
        }
        match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
        }
    }

    pub fn dark() -> Self {
        Self {
            surface: rgb(0x16161c).into(),
            panel: rgb(0x1c1c24).into(),
            elevated: rgb(0x24242e).into(),
            border: rgba(0xffffff14).into(),
            border_strong: rgba(0xffffff26).into(),
            text: rgb(0xe8e8f0).into(),
            text_muted: rgb(0x9a9aae).into(),
            text_faint: rgb(0x62627a).into(),
            hover: rgba(0xffffff0a).into(),
            selected: rgba(0x8b7cf626).into(),
            accent: rgb(0x9d8cff).into(),
            accent_soft: rgba(0x9d8cff33).into(),
            match_fg: rgb(0xffd479).into(),
            match_bg: rgba(0xffd47924).into(),
            text_selection: rgba(0x9d8cff59).into(),
            danger: rgb(0xff6b81).into(),
            shadow: rgba(0x00000099).into(),
            kbd_bg: rgba(0xffffff12).into(),
            link: rgb(0x6cb6ff).into(),
            code: rgb(0x7ee0a1).into(),
            color: rgb(0xff9ad5).into(),
            path: rgb(0xffb86c).into(),
            image: rgb(0x5ee0e6).into(),
        }
    }

    pub fn light() -> Self {
        Self {
            surface: rgb(0xfbfbfd).into(),
            panel: rgb(0xf3f3f7).into(),
            elevated: rgb(0xffffff).into(),
            border: rgba(0x0000001a).into(),
            border_strong: rgba(0x0000002e).into(),
            text: rgb(0x1d1d27).into(),
            text_muted: rgb(0x5f5f73).into(),
            text_faint: rgb(0x9a9aab).into(),
            hover: rgba(0x0000000a).into(),
            selected: rgba(0x6e5cf21f).into(),
            accent: rgb(0x6e5cf2).into(),
            accent_soft: rgba(0x6e5cf226).into(),
            match_fg: rgb(0xa15c00).into(),
            match_bg: rgba(0xffb02e33).into(),
            text_selection: rgba(0x6e5cf247).into(),
            danger: rgb(0xd92d4b).into(),
            shadow: rgba(0x0000003d).into(),
            kbd_bg: rgba(0x0000000d).into(),
            link: rgb(0x1f6fd1).into(),
            code: rgb(0x16834a).into(),
            color: rgb(0xc2318a).into(),
            path: rgb(0xb3600b).into(),
            image: rgb(0x0b8d94).into(),
        }
    }

    pub fn kind_color(&self, kind: Kind) -> Hsla {
        match kind {
            Kind::Text => self.text_muted,
            Kind::Link => self.link,
            Kind::Color => self.color,
            Kind::Path => self.path,
            Kind::Code => self.code,
            Kind::Image => self.image,
        }
    }
}
