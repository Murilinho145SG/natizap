//! The tray icon as a `StatusNotifierItem`, served over D-Bus on the app's
//! Tokio runtime.

use gpui::{App, AppContext as _, Task};
use ksni::TrayMethods as _;
use ksni::menu::{MenuItem, StandardItem};

use crate::{SIDE, TrayAction, TrayError};

/// The icon while it shows.
pub(crate) type Icon = ksni::Handle<Item>;

pub(crate) struct Item {
    pixmap: ksni::Icon,
    actions: async_channel::Sender<TrayAction>,
}

impl Item {
    fn ask(&self, action: TrayAction) {
        // The app hears these as long as it runs.
        let _ = self.actions.try_send(action);
    }
}

impl ksni::Tray for Item {
    fn id(&self) -> String {
        "natizap".into()
    }

    fn title(&self) -> String {
        "natizap".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![self.pixmap.clone()]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.ask(TrayAction::Open);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Open natizap".into(),
                activate: Box::new(|item: &mut Self| item.ask(TrayAction::Open)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit natizap".into(),
                activate: Box::new(|item: &mut Self| item.ask(TrayAction::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub(crate) fn show(
    rgba: Vec<u8>,
    actions: async_channel::Sender<TrayAction>,
    cx: &mut App,
) -> Task<Result<Icon, TrayError>> {
    let side = i32::try_from(SIDE).unwrap_or(i32::MAX);
    let mut argb = rgba;
    // StatusNotifierItem pixmaps are ARGB in network byte order.
    for pixel in argb.as_chunks_mut::<4>().0 {
        pixel.rotate_right(1);
    }
    let item = Item {
        pixmap: ksni::Icon {
            width: side,
            height: side,
            data: argb,
        },
        actions,
    };
    let spawning = gpui_tokio::Tokio::spawn(cx, async move { item.spawn().await });
    cx.background_spawn(async move {
        match spawning.await {
            Ok(Ok(icon)) => Ok(icon),
            Ok(Err(error)) => Err(TrayError::Unavailable(error.to_string())),
            Err(error) => Err(TrayError::Unavailable(error.to_string())),
        }
    })
}
