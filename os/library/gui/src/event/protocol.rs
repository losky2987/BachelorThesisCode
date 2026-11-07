use alloc::string::String;
use serde::{Deserialize, Serialize};

use crate::event::events::GUIEvent;

const GUI_EVENT_PROTOCOL_VERSION: &str = "1.0";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GUIEventProtocol {
    pub name: String,
    pub version: String,
    pub app_id: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_id: Option<usize>,
    pub event: GUIEvent,
}

impl GUIEventProtocol {
    pub fn new(app_id: usize, window_id: Option<usize>, event: GUIEvent) -> Self {
        GUIEventProtocol {
            name: "GUIEventProtocol".into(),
            version: GUI_EVENT_PROTOCOL_VERSION.into(),
            app_id,
            window_id,
            event,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap()
    }

    pub fn is_supported(&self) -> bool {
        self.name == "GUIEventProtocol" && self.version == GUI_EVENT_PROTOCOL_VERSION
    }

    /// Validate the payload identity against the trusted EventPipe route.
    ///
    /// `app_id` comes from the process created by the Window Manager, while
    /// `window_id` comes from the surface assigned at launch. Neither identity
    /// is accepted solely because it appeared in the JSON payload.
    pub fn matches_route(&self, app_id: usize, window_id: usize) -> bool {
        if !self.is_supported() || self.app_id != app_id {
            return false;
        }

        match &self.event {
            GUIEvent::GUI_init { app_id: event_app_id } | GUIEvent::GUI_exit { app_id: event_app_id } => {
                *event_app_id == app_id && self.window_id == Some(window_id)
            }
            GUIEvent::CloseRequested | GUIEvent::WindowTitle { .. } => self.window_id == Some(window_id),
            GUIEvent::WindowManagerMessage { .. } => self.window_id.is_none(),
        }
    }
}

pub fn from_json(json_str: &str) -> Result<GUIEventProtocol, serde_json::Error> {
    serde_json::from_str(json_str)
}
