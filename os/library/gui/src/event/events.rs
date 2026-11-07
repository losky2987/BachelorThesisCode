#![allow(nonstandard_style)]

use serde::{Deserialize, Serialize};

use crate::api::WindowManagerMessage;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(tag = "type")]
pub enum GUIEvent {
    GUI_init {
        app_id: usize,
    },
    GUI_exit {
        app_id: usize,
    },

    /// Window Manager -> application process lifecycle event. Raw input and
    /// geometry use the per-window shared-memory input queue instead.
    CloseRequested,

    /// Optional application-provided title. `None` restores the application
    /// name maintained by the Window Manager.
    WindowTitle {
        title: Option<alloc::string::String>,
    },

    /// Window Manager command delivered over EventPipe.
    WindowManagerMessage {
        message: WindowManagerMessage,
    },
}
