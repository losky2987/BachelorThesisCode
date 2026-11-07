use alloc::{rc::Rc, vec::Vec};
use concurrent::thread;
use drawer::rect_data::RectData;
use spin::RwLock;

use crate::{
    api::{component_frame_for_window, current_component_frame, defer_surface_updates as set_deferred_updates, set_screen_resolution},
    event::{events::GUIEvent, protocol::GUIEventProtocol},
    drawer::component_tree::ComponentTree,
};

use super::{
    event_channel::{app_events_pending, init_process_app_channel, post_event_to_wm, process_app_id, send_event_to_app, take_events_for_app},
    registry::{init_api, with_api},
};

pub(crate) fn defer_surface_updates(defer: bool) {
    set_deferred_updates(defer);
}

pub fn register_surface_for_thread(
    thread_id: usize, app_id: usize, workspace_index: usize, window_id: usize, abs_pos: RectData, component_frame: Rc<RwLock<ComponentTree>>,
) {
    with_api(|api| api.register_surface(thread_id, app_id, workspace_index, window_id, abs_pos, component_frame));
}

/// Initialize the application-owned side of a GUI surface in a new process.
pub fn register_process_surface(
    app_id: usize, workspace_index: usize, window_id: usize, input_queue_generation: u64, abs_pos: RectData, screen_resolution: (u32, u32),
) -> Result<crate::api::ComponentFrameHandle, &'static str> {
    set_screen_resolution(screen_resolution);
    init_api();

    if !init_process_app_channel(app_id, window_id) {
        return Err("Failed to start GUI process event listener");
    }

    let thread_id = thread::current().map(|thread| thread.id()).ok_or("Current thread not found")?;
    let component_frame = Rc::new(RwLock::new(ComponentTree::new(app_id, window_id, input_queue_generation, abs_pos)));
    register_surface_for_thread(thread_id, app_id, workspace_index, window_id, abs_pos, component_frame);

    post_event_to_wm(app_id, GUIEventProtocol::new(app_id, Some(window_id), GUIEvent::GUI_init { app_id })).map_err(|error| error.as_str())?;

    current_component_frame()
}

pub fn run_loop_iter_fns() {
    // User callbacks may call GUI API functions. Move them out before running
    // so no callback can recursively deadlock on the global GuiApi mutex.
    let callbacks = with_api(|api| core::mem::take(&mut api.on_loop_iter_fns));
    let mut dirty_windows: Vec<usize> = Vec::new();

    for data in &callbacks {
        if (data.fun)() && !dirty_windows.contains(&data.window_data.window_id) {
            dirty_windows.push(data.window_data.window_id);
        }
    }

    let frames = with_api(|api| {
        // Preserve callbacks registered by a callback while the lock was free.
        let newly_registered = core::mem::take(&mut api.on_loop_iter_fns);
        api.on_loop_iter_fns = callbacks;
        api.on_loop_iter_fns.extend(newly_registered);

        // Visit every registered frame. update_frame() returns immediately
        // for a clean tree, while a dirty tree is retried even when the
        // callback forgot to return true or a previous FrameCache write failed.
        api.handles
            .values()
            .map(|handle| (handle.window_id, handle.component_frame.clone()))
            .collect::<Vec<_>>()
    });

    // Rendering and EventPipe notification may block. Never keep the global
    // GuiApi mutex held across component locks or IPC.
    for (window_id, component_frame) in frames {
        let mut component_frame = component_frame.write();
        if dirty_windows.contains(&window_id) {
            component_frame.mark_dirty();
        }
        let _ = component_frame.update_frame();
    }
}

pub fn close_process_surface(app_id: usize, window_id: usize) -> Result<(), &'static str> {
    send_event_to_process(app_id, window_id, GUIEvent::CloseRequested)
}

/// Process pending WM -> APP events on the application main thread.
/// Returns false when the Window Manager requests process shutdown.
pub fn run_process_event_iter() -> bool {
    let Some(app_id) = process_app_id() else {
        return true;
    };
    let Ok(handle) = current_component_frame() else {
        return true;
    };
    let window_id = handle.window_id;

    // Raw input and geometry are consumed directly from the per-window shared
    // queue even when there is no EventPipe control traffic.
    if let Some(frame) = component_frame_for_window(window_id) {
        let rect = {
            let mut frame = frame.write();
            frame.process_pending_input();
            frame.rect()
        };
        with_api(|api| api.update_surface_rect(window_id, rect));
    }

    // The FIFO listener records the affected app ID in the global pending set.
    // Avoid touching the mailbox when this process has no EventPipe work.
    if !app_events_pending(app_id) {
        return true;
    }

    for protocol in take_events_for_app(app_id) {
        if let GUIEvent::CloseRequested = protocol.event {
            // GUI_exit is terminal and has a reserved mailbox slot. Retry any
            // transient publication failure rather than consuming the close
            // request and exiting without a lifecycle notification.
            loop {
                if post_event_to_wm(app_id, GUIEventProtocol::new(app_id, Some(window_id), GUIEvent::GUI_exit { app_id })).is_ok() {
                    return false;
                }
                thread::sleep(1);
            }
        }
    }

    true
}

fn send_event_to_process(app_id: usize, window_id: usize, event: GUIEvent) -> Result<(), &'static str> {
    send_event_to_app(app_id, GUIEventProtocol::new(app_id, Some(window_id), event))
        .map(|_| ())
        .map_err(|error| error.as_str())
}
