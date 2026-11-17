use std::borrow::Cow;

use gpui::{App, AssetSource, SharedString};

const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/figtree/Figtree-Regular.ttf"),
    include_bytes!("../assets/fonts/figtree/Figtree-Medium.ttf"),
    include_bytes!("../assets/fonts/figtree/Figtree-SemiBold.ttf"),
    include_bytes!("../assets/fonts/figtree/Figtree-Bold.ttf"),
    include_bytes!("../assets/fonts/bricolage/BricolageGrotesque-SemiBold.ttf"),
    include_bytes!("../assets/fonts/bricolage/BricolageGrotesque-Bold.ttf"),
    include_bytes!("../assets/fonts/emoji/NatizapEmoji.ttf"),
    include_bytes!("../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
];

/// Declares the icon set once: the enum, its asset paths and its bytes.
macro_rules! icons {
    ($($(#[doc = $doc:literal])* $variant:ident => $file:literal,)*) => {
        /// An icon from the bundled set, for `gpui::svg().path(icon.path())`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Icon {
            $($(#[doc = $doc])* $variant,)*
        }

        impl Icon {
            const ALL: &[Icon] = &[$(Icon::$variant),*];

            /// Asset path served by [`Assets`].
            #[must_use]
            pub fn path(self) -> &'static str {
                match self {
                    $(Icon::$variant => concat!("icons/", $file),)*
                }
            }

            fn bytes(self) -> &'static [u8] {
                match self {
                    $(Icon::$variant => include_bytes!(concat!("../assets/icons/", $file)),)*
                }
            }
        }
    };
}

icons! {
    /// Something went wrong, e.g. a message was not sent.
    Alert => "alert.svg",
    /// Archived chats.
    Archive => "archive.svg",
    /// Go back to the previous screen.
    Back => "back.svg",
    /// Sent.
    Check => "check.svg",
    /// Delivered, or read when tinted.
    Checks => "checks.svg",
    /// Opens a menu below it.
    ChevronDown => "chevron-down.svg",
    /// Waiting, e.g. a message still sending.
    Clock => "clock.svg",
    /// A photo.
    Camera => "camera.svg",
    /// A conversation.
    Chats => "chats.svg",
    /// Close the window.
    Close => "close.svg",
    /// Copy to the clipboard.
    Copy => "copy.svg",
    /// Pick an emoji or react.
    Emoji => "emoji.svg",
    /// A document.
    File => "file.svg",
    /// Forward a message to another chat.
    Forward => "forward.svg",
    /// End-to-end encryption.
    Lock => "lock.svg",
    /// Maximize the window.
    Maximize => "maximize.svg",
    /// A voice message.
    Mic => "mic.svg",
    /// Minimize the window.
    Minimize => "minimize.svg",
    /// Notifications silenced.
    Muted => "muted.svg",
    /// Pause what is playing.
    Pause => "pause.svg",
    /// Pinned to the top.
    Pin => "pin.svg",
    /// Play a video or audio.
    Play => "play.svg",
    /// Add something, e.g. attach a file.
    Plus => "plus.svg",
    /// Reply to a message.
    Reply => "reply.svg",
    /// Restore a maximized window.
    Restore => "restore.svg",
    /// Search.
    Search => "search.svg",
    /// Send a message.
    Send => "send.svg",
    /// A starred message.
    Star => "star.svg",
    /// Delete.
    Trash => "trash.svg",
    /// A business WhatsApp vouches for.
    Verified => "verified.svg",
    /// A video.
    Video => "video.svg",
    /// Sound: marks voice notes.
    Waveform => "waveform.svg",
}

/// Serves the bundled icons; register with `Application::with_assets`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(Icon::ALL
            .iter()
            .find(|icon| icon.path() == path)
            .map(|icon| Cow::Borrowed(icon.bytes())))
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Icon::ALL
            .iter()
            .map(|icon| icon.path())
            .filter(|name| name.starts_with(path))
            .map(SharedString::new_static)
            .collect())
    }
}

/// Registers the bundled typefaces, so text looks the same on every system.
///
/// # Errors
///
/// Fails if a bundled font cannot be parsed.
pub fn load_fonts(cx: &App) -> anyhow::Result<()> {
    cx.text_system()
        .add_fonts(FONTS.iter().map(|bytes| Cow::Borrowed(*bytes)).collect())
}
