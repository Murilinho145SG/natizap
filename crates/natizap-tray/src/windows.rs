//! The tray icon in Windows's notification area. It lives on the main
//! thread, whose message loop GPUI runs.

use gpui::{App, Task};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

use crate::{SIDE, TrayAction, TrayError};

/// The icon while it shows.
pub(crate) type Icon = tray_icon::TrayIcon;

pub(crate) fn show(
    rgba: Vec<u8>,
    actions: async_channel::Sender<TrayAction>,
    _cx: &mut App,
) -> Task<Result<Icon, TrayError>> {
    Task::ready(build(rgba, actions))
}

fn build(rgba: Vec<u8>, actions: async_channel::Sender<TrayAction>) -> Result<Icon, TrayError> {
    let unavailable = |error: &dyn std::fmt::Display| TrayError::Unavailable(error.to_string());
    let open = MenuItem::new("Open natizap", true, None);
    let quit = MenuItem::new("Quit natizap", true, None);
    let menu = Menu::new();
    menu.append_items(&[&open, &PredefinedMenuItem::separator(), &quit])
        .map_err(|error| unavailable(&error))?;
    let (open_id, quit_id) = (open.id().clone(), quit.id().clone());
    let choosing = actions.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let action = if event.id == open_id {
            TrayAction::Open
        } else if event.id == quit_id {
            TrayAction::Quit
        } else {
            return;
        };
        let _ = choosing.try_send(action);
    }));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            let _ = actions.try_send(TrayAction::Open);
        }
    }));
    let icon = tray_icon::Icon::from_rgba(rgba, SIDE, SIDE).map_err(|error| unavailable(&error))?;
    TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip("natizap")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .map_err(|error| unavailable(&error))
}
