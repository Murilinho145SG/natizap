//! The calls settings page: which ringtone calls coming in ring with, and
//! the microphone, speaker and camera calls use. Choosing a device during
//! a call moves the call to it.

use gpui::prelude::*;
use gpui::{Context, Div, Entity, FontWeight, SharedString, Window, div, px, rgb, svg};
use natizap_playback::{Device, cameras, microphones, speakers};
use natizap_ui::theme::{ActiveTheme as _, Theme};
use natizap_ui::{Icon, separator};

use crate::desk::{CallDesk, DeviceKind};
use crate::ringtone::Ringtone;

/// The devices there are, once listed.
#[derive(Debug, Default)]
struct Listed {
    microphones: Vec<Device>,
    speakers: Vec<Device>,
    cameras: Vec<Device>,
}

/// Changes a [`CallDesk`]'s ringtone and devices: the calls page of the
/// settings window.
#[derive(Debug)]
pub struct CallSettings {
    desk: Entity<CallDesk>,
    /// `None` while the devices are being listed.
    listed: Option<Listed>,
}

impl CallSettings {
    /// Shows and changes `desk`'s ringtone and devices, listing the devices
    /// there are now.
    pub fn new(desk: Entity<CallDesk>, cx: &mut Context<Self>) -> Self {
        cx.observe(&desk, |_, _, cx| cx.notify()).detach();
        // Listing asks the sound server and the kernel: off the UI thread.
        let listing = cx.background_spawn(async {
            Listed {
                microphones: microphones(),
                speakers: speakers(),
                cameras: cameras(),
            }
        });
        cx.spawn(async move |this, cx| {
            let listed = listing.await;
            let _ = this.update(cx, |settings, cx| {
                settings.listed = Some(listed);
                cx.notify();
            });
        })
        .detach();
        Self { desk, listed: None }
    }

    /// A ringtone to choose; choosing it plays it.
    fn chip(&self, ringtone: Ringtone, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = *cx.theme();
        let selected = self.desk.read(cx).ringtone() == ringtone;
        let tint = if selected { theme.accent } else { theme.text };
        div()
            .id(SharedString::from(format!("ringtone-{}", ringtone.key())))
            .flex()
            .items_center()
            .gap_1p5()
            .px_3()
            .py_1p5()
            .rounded_full()
            .border_1()
            .border_color(rgb(if selected { theme.accent } else { theme.line }))
            .text_size(px(13.0))
            .text_color(rgb(tint))
            .cursor_pointer()
            .hover(|chip| chip.bg(rgb(theme.raised)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.desk
                    .update(cx, |desk, cx| desk.set_ringtone(ringtone, cx));
            }))
            .child(
                svg()
                    .path(Icon::Play.path())
                    .size(px(11.0))
                    .text_color(rgb(tint)),
            )
            .child(ringtone.name())
    }

    /// The `kind` of device to choose from `devices`, the system's own
    /// first.
    fn devices(&self, kind: DeviceKind, devices: &[Device], cx: &mut Context<Self>) -> Div {
        let theme = *cx.theme();
        let chosen = {
            let chosen = self.desk.read(cx).devices();
            match kind {
                DeviceKind::Microphone => chosen.microphone.clone(),
                DeviceKind::Speaker => chosen.speaker.clone(),
                DeviceKind::Camera => chosen.camera.clone(),
            }
        };
        // A chosen device that is gone is not on the list: the system's own
        // is used, and shows as chosen.
        let present = chosen
            .as_ref()
            .filter(|key| devices.iter().any(|device| &device.key == *key));
        let rows = std::iter::once((None, "System default".to_owned()))
            .chain(
                devices
                    .iter()
                    .map(|device| (Some(device.key.clone()), device.name.clone())),
            )
            .enumerate()
            .map(|(index, (key, name))| {
                let selected = key.as_ref() == present;
                Self::device_row(kind, index, key, name, selected, theme, cx)
            });
        section(kind.heading(), theme).child(div().flex().flex_col().children(rows))
    }

    fn device_row(
        kind: DeviceKind,
        index: usize,
        key: Option<String>,
        name: String,
        selected: bool,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        div()
            .id(SharedString::from(format!("{}-{index}", kind.heading())))
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1p5()
            .rounded_md()
            .cursor_pointer()
            .hover(|row| row.bg(rgb(theme.raised)))
            .text_size(px(13.5))
            .when(selected, |row| row.text_color(rgb(theme.accent)))
            .on_click(cx.listener(move |this, _, _, cx| {
                let key = key.clone();
                this.desk
                    .update(cx, |desk, cx| desk.set_device(kind, key, cx));
            }))
            .child(div().size(px(14.0)).flex_none().when(selected, |slot| {
                slot.child(
                    svg()
                        .path(Icon::Check.path())
                        .size(px(14.0))
                        .text_color(rgb(theme.accent)),
                )
            }))
            .child(div().min_w_0().truncate().child(name))
    }
}

impl DeviceKind {
    fn heading(self) -> &'static str {
        match self {
            Self::Microphone => "Microphone",
            Self::Speaker => "Speaker",
            Self::Camera => "Camera",
        }
    }
}

/// A section of the page, under its heading.
fn section(heading: &'static str, theme: Theme) -> Div {
    div().flex().flex_col().gap_2().px_5().py_3().child(
        div()
            .text_size(px(13.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(theme.muted))
            .child(heading),
    )
}

impl Render for CallSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let chips: Vec<_> = Ringtone::ALL
            .into_iter()
            .map(|ringtone| self.chip(ringtone, cx).into_any_element())
            .collect();
        let ringtone = section("Ringtone", theme)
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(rgb(theme.muted))
                    .child("What a call coming in rings with. Choosing one plays it."),
            )
            .child(div().flex().flex_wrap().gap_2().children(chips));
        let devices = match self.listed.take() {
            Some(listed) => {
                let sections = [
                    self.devices(DeviceKind::Microphone, &listed.microphones, cx),
                    self.devices(DeviceKind::Speaker, &listed.speakers, cx),
                    self.devices(DeviceKind::Camera, &listed.cameras, cx),
                ];
                self.listed = Some(listed);
                div().flex().flex_col().children(sections)
            }
            None => div()
                .px_5()
                .py_3()
                .text_size(px(12.5))
                .text_color(rgb(theme.muted))
                .child("Looking for devices…"),
        };
        div()
            .flex()
            .flex_col()
            .child(ringtone)
            .child(div().px_5().py_1().child(separator(theme)))
            .child(devices)
    }
}
