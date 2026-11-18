//! Your own profile: your picture, name, about line and number, each of
//! them changed from here on all your devices, as the phone's profile
//! screen does.

mod field;
mod picture;
mod render;

use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable, Image, KeyDownEvent, PathPromptOptions,
    Pixels, Point, SharedString, Window,
};
use natizap_profiles::ProfilePictures;
use natizap_protocol::{ActionError, Me, Protocol};
use natizap_ui::{Submitted, TextInput};

use crate::panel::image;
use field::Field;

/// Side of the picture at the top.
const PICTURE: f32 = 176.0;

/// The user went back from their profile to the chats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnProfileClosed;

/// A line being written.
struct Editing {
    field: Field,
    input: Entity<TextInput>,
}

/// Your own profile, shown in place of the chat list.
pub struct OwnProfile {
    protocol: Entity<Protocol>,
    pictures: Entity<ProfilePictures>,
    me: Me,
    picture: Option<Arc<Image>>,
    /// Your about line, when you have one.
    about: Option<String>,
    /// Whether the about line was read yet.
    about_read: bool,
    editing: Option<Editing>,
    /// A line being saved, shown dimmed until WhatsApp takes it.
    saving: Option<(Field, String)>,
    /// Why the last change of a line, or of the picture, failed.
    error: Option<(Option<Field>, SharedString)>,
    /// A new picture being made and sent.
    uploading: bool,
    /// Where the picture's menu is open.
    picture_menu: Option<Point<Pixels>>,
    focus: FocusHandle,
}

impl std::fmt::Debug for OwnProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnProfile")
            .field("me", &self.me)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<OwnProfileClosed> for OwnProfile {}

impl Focusable for OwnProfile {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl OwnProfile {
    /// Shows `me`'s profile, fetching the full-size picture and the about
    /// line.
    pub fn new(
        protocol: &Entity<Protocol>,
        pictures: &Entity<ProfilePictures>,
        me: Me,
        cx: &mut Context<Self>,
    ) -> Self {
        let profile = Self {
            protocol: protocol.clone(),
            pictures: pictures.clone(),
            me,
            picture: None,
            about: None,
            about_read: false,
            editing: None,
            saving: None,
            error: None,
            uploading: false,
            picture_menu: None,
            focus: cx.focus_handle(),
        };
        profile.fetch_picture(cx);
        let reading = protocol.read(cx).profile(&profile.me.phone, cx);
        cx.spawn(async move |this, cx| {
            let about = match reading.await {
                Ok(profile) => profile.about,
                Err(error) => {
                    tracing::debug!(%error, "own about line unavailable");
                    None
                }
            };
            let _ = this.update(cx, |profile, cx| {
                profile.about = about;
                profile.about_read = true;
                cx.notify();
            });
        })
        .detach();
        profile
    }

    fn fetch_picture(&self, cx: &mut Context<Self>) {
        let fetching = self
            .protocol
            .read(cx)
            .profile_picture(&self.me.phone, true, cx);
        cx.spawn(async move |this, cx| {
            let picture = match fetching.await {
                Ok(picture) => picture,
                Err(error) => {
                    tracing::debug!(%error, "no full-size own picture");
                    return;
                }
            };
            let _ = this.update(cx, |profile, cx| {
                profile.picture = picture.map(image);
                cx.notify();
            });
        })
        .detach();
    }

    /// What `field` says now.
    fn value(&self, field: Field) -> Option<String> {
        match field {
            Field::Name => self.me.name.clone(),
            Field::About => self.about.clone(),
        }
    }

    fn start_editing(&mut self, field: Field, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.value(field).unwrap_or_default();
        let input = cx.new(|cx| {
            let input = TextInput::new(field.label(), cx);
            match field {
                Field::Name => input,
                Field::About => input.wrapping(),
            }
        });
        input.update(cx, |input, cx| input.insert(&text, window, cx));
        cx.subscribe(&input, move |profile, _, Submitted(text), cx| {
            profile.save(field, text, cx);
        })
        .detach();
        window.focus(&input.focus_handle(cx), cx);
        self.editing = Some(Editing { field, input });
        self.error = None;
        cx.notify();
    }

    fn stop_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// Saves `text` as `field` on all your devices.
    fn save(&mut self, field: Field, text: &str, cx: &mut Context<Self>) {
        let text = match field.check(text) {
            Ok(text) => text,
            Err(why) => {
                self.error = Some((Some(field), why.into()));
                cx.notify();
                return;
            }
        };
        self.editing = None;
        if self.value(field).as_deref() == Some(text.as_str()) {
            cx.notify();
            return;
        }
        let protocol = self.protocol.read(cx);
        let saving = match field {
            Field::Name => protocol.set_own_name(text.clone(), cx),
            Field::About => protocol.set_own_about(text.clone(), cx),
        };
        self.saving = Some((field, text.clone()));
        self.error = None;
        cx.spawn(async move |this, cx| {
            let saved = saving.await;
            let _ = this.update(cx, |profile, cx| {
                profile.saving = None;
                match saved {
                    Ok(()) => match field {
                        Field::Name => profile.me.name = Some(text),
                        Field::About => profile.about = Some(text),
                    },
                    Err(error) => profile.failed(Some(field), &error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn failed(&mut self, field: Option<Field>, error: &ActionError) {
        tracing::warn!(%error, "could not change the own profile");
        let what = field.map_or("the picture", |field| match field {
            Field::Name => "your name",
            Field::About => "your about",
        });
        self.error = Some((field, format!("Could not change {what}: {error}").into()));
    }

    /// Asks for a photo, then makes it your picture, cut square.
    fn upload_picture(&mut self, cx: &mut Context<Self>) {
        self.picture_menu = None;
        let picking = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            let path: PathBuf = match picking.await {
                Ok(Ok(Some(mut paths))) if !paths.is_empty() => paths.swap_remove(0),
                Ok(Ok(_)) | Err(_) => return,
                Ok(Err(error)) => {
                    tracing::warn!(%error, "could not ask for a photo");
                    return;
                }
            };
            let _ = this.update(cx, |profile, cx| {
                profile.uploading = true;
                profile.error = None;
                cx.notify();
            });
            let made = cx
                .background_spawn(async move {
                    let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
                    picture::square_jpeg(&bytes).map_err(|error| error.to_string())
                })
                .await;
            let jpeg = match made {
                Ok(jpeg) => jpeg,
                Err(error) => {
                    let _ = this.update(cx, |profile, cx| {
                        profile.uploading = false;
                        tracing::warn!(%error, "could not read the chosen photo");
                        profile.error =
                            Some((None, "That file is not a photo natizap can read".into()));
                        cx.notify();
                    });
                    return;
                }
            };
            let Ok(sending) = this.update(cx, |profile, cx| {
                profile.protocol.read(cx).set_own_picture(jpeg.clone(), cx)
            }) else {
                return;
            };
            let sent = sending.await;
            let _ = this.update(cx, |profile, cx| {
                profile.uploading = false;
                match sent {
                    Ok(()) => {
                        profile.picture = Some(image(jpeg));
                        profile.refresh_avatar(cx);
                    }
                    Err(error) => profile.failed(None, &error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn remove_picture(&mut self, cx: &mut Context<Self>) {
        self.picture_menu = None;
        self.uploading = true;
        let removing = self.protocol.read(cx).remove_own_picture(cx);
        cx.spawn(async move |this, cx| {
            let removed = removing.await;
            let _ = this.update(cx, |profile, cx| {
                profile.uploading = false;
                match removed {
                    Ok(()) => {
                        profile.picture = None;
                        profile.refresh_avatar(cx);
                    }
                    Err(error) => profile.failed(None, &error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Has the small picture everywhere else show the change.
    fn refresh_avatar(&self, cx: &mut Context<Self>) {
        let phone = self.me.phone.clone();
        self.pictures
            .update(cx, |pictures, cx| pictures.refresh(&phone, cx));
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key != "escape" {
            return;
        }
        // Escape leaves the line being written, then the profile.
        if self.editing.is_some() {
            self.stop_editing(window, cx);
        } else {
            cx.emit(OwnProfileClosed);
        }
        cx.stop_propagation();
    }
}
