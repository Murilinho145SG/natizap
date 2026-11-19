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
    /// Something comes down, e.g. into the drop tray.
    ArrowDown => "arrow-down.svg",
    /// Go back to the previous screen.
    Back => "back.svg",
    /// Notifications and how they sound.
    Bell => "bell.svg",
    /// Block someone, or unblock them.
    Block => "block.svg",
    /// Sent.
    Check => "check.svg",
    /// Delivered, or read when tinted.
    Checks => "checks.svg",
    /// Opens a menu below it.
    ChevronDown => "chevron-down.svg",
    /// Back to an earlier item above, e.g. the previous search hit.
    ChevronUp => "chevron-up.svg",
    /// Back to an earlier item, e.g. the previous photo.
    ChevronLeft => "chevron-left.svg",
    /// On to a later item, e.g. the next photo.
    ChevronRight => "chevron-right.svg",
    /// Waiting, e.g. a message still sending.
    Clock => "clock.svg",
    /// A photo.
    Camera => "camera.svg",
    /// A phone call to make, e.g. a business's call button.
    Call => "call.svg",
    /// Ends a call.
    CallEnd => "call-end.svg",
    /// A call nobody answered.
    CallMissed => "call-missed.svg",
    /// A conversation.
    Chats => "chats.svg",
    /// Clear a chat's messages, keeping the chat.
    Clear => "clear.svg",
    /// Close the window.
    Close => "close.svg",
    /// A person, e.g. a shared contact.
    Contact => "contact.svg",
    /// Copy to the clipboard.
    Copy => "copy.svg",
    /// Download a file the server holds, e.g. an old sticker.
    Download => "download.svg",
    /// Pick an emoji or react.
    Emoji => "emoji.svg",
    /// Change something written, e.g. your name.
    Edit => "edit.svg",
    /// A shared or live location.
    Location => "location.svg",
    /// A document.
    File => "file.svg",
    /// Forward a message to another chat.
    Forward => "forward.svg",
    /// A web link.
    Link => "link.svg",
    /// End-to-end encryption.
    Lock => "lock.svg",
    /// natizap's mark: an "n" that is also a speech bubble.
    Logo => "logo.svg",
    /// Fills the whole screen.
    FullScreen => "fullscreen.svg",
    /// Leaves the whole screen.
    FullScreenExit => "fullscreen-exit.svg",
    /// Shows things side by side, in a grid.
    Grid => "grid.svg",
    /// Maximize the window.
    Maximize => "maximize.svg",
    /// A voice message.
    Mic => "mic.svg",
    /// The microphone silenced in a call.
    MicOff => "mic-off.svg",
    /// Minimize the window.
    Minimize => "minimize.svg",
    /// More actions, in a menu.
    More => "more.svg",
    /// Notifications silenced.
    Muted => "muted.svg",
    /// Chooses where something shows in the window.
    Layout => "layout.svg",
    /// A voice call that came in.
    CallIncoming => "call-incoming.svg",
    /// A voice call that was made.
    CallOutgoing => "call-outgoing.svg",
    /// A video call that came in.
    VideoIncoming => "video-incoming.svg",
    /// A video call that was made.
    VideoOutgoing => "video-outgoing.svg",
    /// Moves something into a window of its own.
    PopOut => "pop-out.svg",
    /// Shares the screen in a call.
    ScreenShare => "screen-share.svg",
    /// How the app looks: its theme.
    Palette => "palette.svg",
    /// Pause what is playing.
    Pause => "pause.svg",
    /// Pinned to the top.
    Pin => "pin.svg",
    /// Play a video or audio.
    Play => "play.svg",
    /// Add something, e.g. attach a file.
    Plus => "plus.svg",
    /// A poll.
    Poll => "poll.svg",
    /// Reply to a message.
    Reply => "reply.svg",
    /// Restore a maximized window.
    Restore => "restore.svg",
    /// Search.
    Search => "search.svg",
    /// Send a message.
    Send => "send.svg",
    /// Settings.
    Settings => "settings.svg",
    /// A sticker.
    Sticker => "sticker.svg",
    /// A starred message.
    /// Status updates, the place that shows them.
    Status => "status.svg",
    Star => "star.svg",
    /// Where dropped files land.
    Tray => "tray.svg",
    /// Delete.
    Trash => "trash.svg",
    /// Mark a chat as unread.
    Unread => "unread.svg",
    /// A business WhatsApp vouches for.
    Verified => "verified.svg",
    /// A video.
    Video => "video.svg",
    /// The camera turned off in a call.
    VideoOff => "video-off.svg",
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
