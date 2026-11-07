use alloc::string::String;
use serde::{Deserialize, Serialize};

use crate::{
    event::{events::GUIEvent, protocol::GUIEventProtocol},
    runtime::event_channel::{post_event_to_wm, process_app_id},
};

use super::current_component_frame;

const LOCAL_WINDOW_MANAGER_ROUTE: usize = usize::MAX >> 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ScreenSplitType {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum WindowManagerMessage {
    CreateNewWorkspace,
    CloseCurrentWorkspace,
    SwitchToWorkspace(usize),

    CloseCurrentWindow,
    MoveCurrentWindowForward,
    MoveCurrentWindowBackward,

    LaunchApp(String, ScreenSplitType),
}

pub fn send_window_manager_message(message: WindowManagerMessage) -> Result<(), &'static str> {
    // WM chrome/workspace buttons run in the Window Manager process and have
    // no app_id. A dedicated route avoids collisions with a real process ID;
    // releasing an app route must never erase a pending local button action.
    if process_app_id().is_some() {
        return Err("Window Manager commands are restricted to the Window Manager process");
    }
    let app_id = LOCAL_WINDOW_MANAGER_ROUTE;
    post_event_to_wm(app_id, GUIEventProtocol::new(app_id, None, GUIEvent::WindowManagerMessage { message }))
        .map(|_| ())
        .map_err(|error| error.as_str())
}

/// Set the title shown in this window's action bar. Passing None restores
/// the executable/application name supplied by the Window Manager.
pub fn set_window_title(title: Option<String>) -> Result<(), &'static str> {
    let handle = current_component_frame()?;
    post_event_to_wm(
        handle.app_id,
        GUIEventProtocol::new(handle.app_id, Some(handle.window_id), GUIEvent::WindowTitle { title }),
    )
    .map(|_| ())
    .map_err(|error| error.as_str())
}
