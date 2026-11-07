//! Public application-facing GUI API.

mod component_frame;
mod registration;
mod window;

pub(crate) use self::component_frame::defer_surface_updates;
pub use self::component_frame::{
    ComponentFrameHandle, LOG_SCREEN, component_ref, next_component_id, scale_radius_with_ratios, scale_rect_data_with_ratios, scale_vertex_with_ratios,
    screen_resolution, set_screen_resolution,
};
pub use self::registration::{component_frame_for_window, current_component_frame, register_current_loop_iter};
pub use self::window::{ScreenSplitType, WindowManagerMessage, send_window_manager_message, set_window_title};

// Compatibility exports for callers that used runtime operations through
// gui::api before the runtime/API split.
pub use crate::runtime::{
    GuiApi, HandleData, NewLoopIterFnData, WindowData, close_process_surface, get_api, init_api, init_process_event_channel, monitor_process_exit,
    register_process_surface, register_surface_for_thread, release_process_state, run_loop_iter_fns, run_process_event_iter, take_events_for_wm,
    take_pending_events_for_wm, with_api,
};
