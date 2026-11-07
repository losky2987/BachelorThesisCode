#![no_std]
#![feature(linked_list_cursors)]
// TODO: There are a lot of warnings in the window manager and they should be fixed in the future!
//       I disabled them for now, because they clutter up the console,
//       making it hard to see new warnings/errors - Fabian Ruhland (21.10.2025)
#![allow(unused_variables)]
#![allow(dead_code)]
#![allow(static_mut_refs)]

extern crate alloc;

use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use chrono::TimeDelta;
use concurrent::{process, thread};
use config::{COMMAND_LINE_WINDOW_Y_PADDING, DIST_TO_SCREEN_EDGE, FLUSHING_DELAY_MS};
use drawer::drawer::Drawer;
use drawer::rect_data::RectData;
use drawer::vertex::Vertex;
use globals::hotkeys::HKEY_TOGGLE_TERMINAL_WINDOW;
use graphic::lfb::DEFAULT_CHAR_HEIGHT;
#[cfg(feature = "with_runtime")]
use gui::api::set_screen_resolution;
use gui::event::events::GUIEvent;
use gui::event::protocol::GUIEventProtocol;
use gui::{
    api::{ScreenSplitType, WindowManagerMessage, screen_resolution},
    runtime::{
        child_proc::{ChildProcess, run_gapp},
        close_process_surface, init_api, init_process_event_channel, monitor_process_exit, release_process_state, take_events_for_wm,
        take_pending_events_for_wm,
    },
};

use input::mouse::try_read_mouse;
#[allow(unused_imports)]
#[cfg(feature = "with_runtime")]
use runtime::*;
use terminal::{DecodedKey, println};
use time::systime;
use window::workspace_selection::WorkspaceSelectionWindow;
use window::{app::AppWindow, command_line::CommandLineWindow};
use workspace::Workspace;

use crate::input_agent::{keyboard::KeyboardDecoder, mouse::MouseState};
use gui::component::assistant::config;

mod input_agent;
mod window;
mod workspace;

const WORKSPACE_SELECTION_WINDOW_HEIGHT: u32 = 30;
const MIN_WINDOW_WIDTH: u32 = 120;
const MIN_WINDOW_HEIGHT: u32 = window::app::ACTION_BAR_HEIGHT + 40;
// GUI runtime pending-ID registries have 128 slots. Keep headroom for the
// Window Manager's own route and teardown races; route registration now fails
// explicitly instead of leaving an enqueued event without a wake-up marker.
const MAX_GUI_WINDOWS: usize = 96;

/// Default app to be used on startup of a new workspace.
pub static DEFAULT_APP: &str = "editor";

struct WindowManager {
    workspaces: Vec<Workspace>,
    current_workspace: usize,
    workspace_selection_window: WorkspaceSelectionWindow,
    command_line_window: CommandLineWindow,
    is_dirty: bool,
    last_frame_time: TimeDelta,
    start_time: TimeDelta,
    frames: i64,
    mouse_state: MouseState,
    keyboard_decoder: KeyboardDecoder,
}

impl WindowManager {
    fn new() -> Self {
        let screen = screen_resolution();

        let workspace_selection_window = WorkspaceSelectionWindow::new(RectData {
            top_left: Vertex::new(DIST_TO_SCREEN_EDGE, DIST_TO_SCREEN_EDGE),
            width: screen.0.saturating_sub(DIST_TO_SCREEN_EDGE.saturating_mul(2)),
            height: WORKSPACE_SELECTION_WINDOW_HEIGHT,
        });

        let command_line_window_height = DEFAULT_CHAR_HEIGHT + COMMAND_LINE_WINDOW_Y_PADDING * 2;
        let command_line_window = CommandLineWindow::new(
            RectData {
                top_left: Vertex::new(
                    DIST_TO_SCREEN_EDGE,
                    screen.1.saturating_sub(DIST_TO_SCREEN_EDGE.saturating_add(command_line_window_height)),
                ),
                width: screen.0.saturating_sub(DIST_TO_SCREEN_EDGE.saturating_mul(2)),
                height: command_line_window_height,
            },
            None,
        );

        let time = systime();

        Self {
            workspaces: Vec::new(),
            current_workspace: 0,
            workspace_selection_window,
            command_line_window,
            is_dirty: true,
            last_frame_time: time,
            start_time: time,
            frames: 0,
            mouse_state: MouseState::new(),
            keyboard_decoder: KeyboardDecoder::new(),
        }
    }

    fn init(&mut self) {
        init_api();
        if !self.create_new_workspace() {
            println!("WindowManager: failed to create initial workspace");
            self.enter_text_mode();
        }
    }

    fn run(&mut self) {
        loop {
            self.process_keyboard_input();
            self.process_mouse_input();

            self.process_event_pipe_messages();
            self.poll_component_frames();

            self.draw();
            self.flush();
            self.mouse_state.draw_cursor();

            // Do not monopolize a CPU while all pending-ID gates are empty;
            // application FIFO listeners and newly launched GUI processes need
            // a scheduling opportunity to publish their first events/frame.
            thread::switch();
        }
    }

    fn process_event_pipe_messages(&mut self) {
        let pending_ids = take_pending_events_for_wm();

        for app_id in pending_ids {
            let events = take_events_for_wm(app_id);
            for protocol in events {
                self.handle_gui_event(protocol);
            }
        }
    }

    fn handle_gui_event(&mut self, protocol: GUIEventProtocol) {
        match protocol.event {
            GUIEvent::GUI_init { app_id } => {
                let Some(window_id) = protocol.window_id else {
                    return;
                };
                for workspace in &mut self.workspaces {
                    if let Some(window) = workspace.windows.get_mut(&window_id) {
                        // The syscall-provided pid and the app's self-reported
                        // pid must describe the same route.
                        if window.app_id() == Some(app_id) {
                            window.set_app_id(app_id);
                        }
                        break;
                    }
                }
            }
            GUIEvent::GUI_exit { app_id } => {
                let route = protocol.window_id.and_then(|window_id| {
                    self.workspaces
                        .iter()
                        .position(|workspace| {
                            workspace
                                .windows
                                .get(&window_id)
                                .is_some_and(|window| window.app_id() == Some(app_id))
                        })
                        .map(|workspace_index| (workspace_index, window_id))
                });

                // Tear down the fixed-capacity event route even for a stale or
                // already removed window. GUI_exit can be delivered once by
                // the app and once by its process monitor.
                release_process_state(app_id);

                if let Some((workspace_index, window_id)) = route {
                    self.remove_window(workspace_index, window_id, false);
                }
            }
            GUIEvent::WindowTitle { title } => {
                let Some(window_id) = protocol.window_id else {
                    return;
                };
                for workspace in &mut self.workspaces {
                    if let Some(window) = workspace.windows.get_mut(&window_id) {
                        if window.app_id() == Some(protocol.app_id) {
                            window.set_title(title);
                            self.is_dirty = true;
                        }
                        break;
                    }
                }
            }
            GUIEvent::WindowManagerMessage { message } => {
                self.handle_window_manager_message(message);
            }
            _ => {}
        }
    }

    fn handle_window_manager_message(&mut self, message: WindowManagerMessage) {
        match message {
            WindowManagerMessage::CreateNewWorkspace => {
                let _ = self.create_new_workspace();
            }
            WindowManagerMessage::CloseCurrentWorkspace => {
                self.remove_current_workspace();
            }
            WindowManagerMessage::SwitchToWorkspace(id) => {
                if let Some(index) = self.workspaces.iter().position(|workspace| workspace.get_id() == id) {
                    self.switch_workspace(index);
                }
            }
            WindowManagerMessage::CloseCurrentWindow => {
                self.close_current_window();
            }
            WindowManagerMessage::MoveCurrentWindowForward => {
                self.get_current_workspace_mut().move_focused_window_forward();
                self.is_dirty = true;
            }
            WindowManagerMessage::MoveCurrentWindowBackward => {
                self.get_current_workspace_mut().move_focused_window_backward();
                self.is_dirty = true;
            }
            WindowManagerMessage::LaunchApp(app_name, split_type) => {
                if self.is_app_name_valid(&app_name) {
                    let window_id = self.get_current_workspace().focused_window_id;
                    self.split_window(window_id, split_type, &app_name);
                }
            }
        }
    }

    /// Poll every registered graphical window on every compositor iteration.
    /// Only the small shared-memory header is read for a clean frame; pixel
    /// data is copied when the application's completed revision changes.
    fn poll_component_frames(&mut self) {
        for workspace in &mut self.workspaces {
            for window in workspace.windows.values_mut() {
                window.poll_component_frame();
            }
        }
    }

    fn process_keyboard_input(&mut self) {
        let read_option = self.keyboard_decoder.read_decoded();

        if let Some(keyboard_input) = read_option {
            let keyboard_press = keyboard_input.key;

            // F1 is always reserved by the system. When an application owns
            // keyboard focus, Ctrl keeps the remaining Window Manager
            // shortcuts reachable without stealing ordinary letters from the
            // application (for example, Ctrl+H/Ctrl+V and Ctrl+C).
            let always_reserved = matches!(keyboard_press, DecodedKey::RawKey(HKEY_TOGGLE_TERMINAL_WINDOW));
            if (always_reserved || keyboard_input.control_down) && self.hotkey_handle(keyboard_press) {
                return;
            }

            if self.command_line_window.is_active() {
                let keep_enter_app_mode = self.command_line_window.process_keyboard_input(keyboard_press);

                if !keep_enter_app_mode {
                    self.is_dirty = true;
                }

                return;
            }

            let focused_app_id = self.get_current_workspace().get_focused_window().app_id();
            if self.get_current_workspace().get_focused_window().captures_keyboard() {
                if focused_app_id.is_some() {
                    self.get_current_workspace_mut().get_focused_window_mut().send_keyboard_input(keyboard_press);
                }
                return;
            }

            if !self.hotkey_handle(keyboard_press) {
                if focused_app_id.is_some() {
                    self.get_current_workspace_mut().get_focused_window_mut().send_keyboard_input(keyboard_press);
                }
            }
        }
    }

    fn hotkey_handle(&mut self, hotkey: DecodedKey) -> bool {
        let handled = matches!(
            hotkey,
            DecodedKey::RawKey(HKEY_TOGGLE_TERMINAL_WINDOW) | DecodedKey::Unicode('c' | 'x' | 'q' | 'e' | 'o' | 'i' | 'h' | 'v' | 'w' | 's' | 'a' | 'd' | 'm')
        );

        match hotkey {
            DecodedKey::RawKey(HKEY_TOGGLE_TERMINAL_WINDOW) => {
                self.enter_text_mode();
            }
            DecodedKey::Unicode('c') => {
                let _ = self.create_new_workspace();
            }
            DecodedKey::Unicode('x') => {
                self.remove_current_workspace();
            }
            DecodedKey::Unicode('q') => {
                self.switch_prev_workspace();
            }
            DecodedKey::Unicode('e') => {
                self.switch_next_workspace();
            }
            DecodedKey::Unicode('o') => {
                self.get_current_workspace_mut().move_focused_window_forward();
                self.is_dirty = true;
            }
            DecodedKey::Unicode('i') => {
                self.get_current_workspace_mut().move_focused_window_backward();
                self.is_dirty = true;
            }
            DecodedKey::Unicode('h') => {
                self.command_line_window.activate_enter_app_mode(ScreenSplitType::Horizontal);
            }
            DecodedKey::Unicode('v') => {
                self.command_line_window.activate_enter_app_mode(ScreenSplitType::Vertical);
            }
            DecodedKey::Unicode('w') => {
                self.get_current_workspace_mut().focus_next_component();
            }
            DecodedKey::Unicode('s') => {
                self.get_current_workspace_mut().focus_prev_component();
            }
            DecodedKey::Unicode('a') => {
                self.get_current_workspace_mut().focus_prev_window();
            }
            DecodedKey::Unicode('d') => {
                self.get_current_workspace_mut().focus_next_window();
            }
            DecodedKey::Unicode('m') => {
                self.close_current_window();
            }
            _ => {}
        }

        handled
    }

    fn process_mouse_input(&mut self) {
        let mouse_packet = try_read_mouse();
        if let Some(mouse_packet) = mouse_packet {
            let mouse_event = self.mouse_state.process(&mouse_packet);

            if let Some(callback) = self.workspace_selection_window.handle_mouse_event(&mouse_event) {
                callback();
                return;
            }

            let button_pressed = mouse_event.buttons.left.is_pressed()
                || mouse_event.buttons.right.is_pressed()
                || mouse_event.buttons.middle.is_pressed()
                || mouse_event.buttons.button4.is_pressed()
                || mouse_event.buttons.button5.is_pressed();
            if button_pressed {
                self.get_current_workspace_mut().focus_window_at(mouse_event.position);
            }
            if self
                .get_current_workspace_mut()
                .get_focused_window_mut()
                .handle_chrome_mouse_event(&mouse_event)
            {
                return;
            }

            if self.get_current_workspace().get_focused_window().app_id().is_some() {
                self.get_current_workspace_mut().get_focused_window_mut().send_mouse_input(&mouse_event);
            }
        }
    }

    fn add_window_to_workspace(&mut self, rect_data: RectData, app_name: &str) -> bool {
        if self.window_count() >= MAX_GUI_WINDOWS {
            return false;
        }

        let mut window = AppWindow::new(rect_data, app_name);
        let window_id = window.get_id();
        let content_rect = window.content_rect();
        let workspace_index = self.current_workspace;
        let Some(input_queue_generation) = window.prepare_app_surface(content_rect) else {
            return false;
        };

        // Do not publish a window until its EventPipe and process exist. This
        // keeps a failed launch from leaving an unusable half-window behind.
        let Some(child) = self.launch_gui_process(app_name, workspace_index, window_id, input_queue_generation) else {
            return false;
        };
        window.set_app_id(child.app_id());

        let curr_ws = self.get_current_workspace_mut();
        curr_ws.insert_window(window, curr_ws.focused_window_id);

        self.is_dirty = true;
        true
    }

    fn attach_app_to_window(&mut self, window_id: usize, app_name: &str) -> bool {
        let Some(content_rect) = self.get_current_workspace().windows.get(&window_id).map(AppWindow::content_rect) else {
            return false;
        };
        let input_queue_generation = self
            .get_current_workspace_mut()
            .windows
            .get_mut(&window_id)
            .and_then(|window| window.prepare_app_surface(content_rect));
        let Some(input_queue_generation) = input_queue_generation else {
            return false;
        };
        let Some(child) = self.launch_gui_process(app_name, self.current_workspace, window_id, input_queue_generation) else {
            return false;
        };
        if let Some(window) = self.get_current_workspace_mut().windows.get_mut(&window_id) {
            window.set_app_name(app_name);
            window.set_app_id(child.app_id());
            window.mark_window_dirty();
            self.is_dirty = true;
            true
        } else {
            child.terminate();
            false
        }
    }

    fn split_window(&mut self, window_id: usize, split_type: ScreenSplitType, app_name: &str) {
        let curr_ws = self.get_current_workspace_mut();

        if let Some(window) = curr_ws.windows.get_mut(&window_id) {
            if window.app_id().is_none() {
                return;
            }
            let old_top_left = window.rect_data.top_left;
            let old_width = window.rect_data.width;
            let old_height = window.rect_data.height;

            match split_type {
                ScreenSplitType::Horizontal => {
                    if old_height < MIN_WINDOW_HEIGHT.saturating_mul(2) {
                        return;
                    }
                    window.rect_data.height /= 2;
                    let new_top_left = old_top_left.add(0, window.rect_data.height);
                    let new_rect_data = RectData {
                        top_left: new_top_left,
                        width: old_width,
                        height: old_height.saturating_sub(window.rect_data.height),
                    };

                    window.rescale_window_in_place(window.rect_data);
                    window.resize_app_surface(window.content_rect());
                    if !self.add_window_to_workspace(new_rect_data, app_name) {
                        if let Some(window) = self.get_current_workspace_mut().windows.get_mut(&window_id) {
                            window.rescale_window_in_place(RectData {
                                top_left: old_top_left,
                                width: old_width,
                                height: old_height,
                            });
                            window.resize_app_surface(window.content_rect());
                        }
                    }
                }

                ScreenSplitType::Vertical => {
                    if old_width < MIN_WINDOW_WIDTH.saturating_mul(2) {
                        return;
                    }
                    window.rect_data.width /= 2;
                    let new_top_left = old_top_left.add(window.rect_data.width, 0);
                    let new_rect_data = RectData {
                        top_left: new_top_left,
                        width: old_width.saturating_sub(window.rect_data.width),
                        height: old_height,
                    };

                    window.rescale_window_in_place(window.rect_data);
                    window.resize_app_surface(window.content_rect());
                    if !self.add_window_to_workspace(new_rect_data, app_name) {
                        if let Some(window) = self.get_current_workspace_mut().windows.get_mut(&window_id) {
                            window.rescale_window_in_place(RectData {
                                top_left: old_top_left,
                                width: old_width,
                                height: old_height,
                            });
                            window.resize_app_surface(window.content_rect());
                        }
                    }
                }
            }
        }
    }

    fn close_current_window(&mut self) {
        let window_id = self.get_current_workspace().focused_window_id;
        self.remove_window(self.current_workspace, window_id, true);
    }

    /// Remove a window without ever retaining its chrome as an app-less
    /// placeholder. A workspace is defined to contain at least one window, so
    /// removing its last window removes that workspace as well. Closing the
    /// final window returns to text mode.
    fn remove_window(
        &mut self,
        workspace_index: usize,
        window_id: usize,
        request_app_close: bool,
    ) -> bool {
        let Some(window) = self
            .workspaces
            .get(workspace_index)
            .and_then(|workspace| workspace.windows.get(&window_id))
        else {
            return false;
        };
        let app_id = window.app_id();

        if request_app_close {
            let Some(app_id) = app_id else {
                return false;
            };
            if close_process_surface(app_id, window_id).is_err() {
                return false;
            }
        }

        if self.workspaces[workspace_index].windows.len() > 1 {
            if !self.workspaces[workspace_index].remove_target_window(window_id) {
                return false;
            }
            self.is_dirty = true;
            return true;
        }

        if self.workspaces.len() > 1 {
            // The target app either already exited or received CloseRequested
            // above; do not enqueue a duplicate while removing its workspace.
            self.remove_workspace(workspace_index, app_id);
            return true;
        }

        self.enter_text_mode();
        true
    }

    fn create_new_workspace(&mut self) -> bool {
        if self.workspaces.len() >= 9 || self.window_count() >= MAX_GUI_WINDOWS {
            return false;
        }

        let new_workspace_index = self.workspaces.len();
        let screen_res = screen_resolution();
        let window_rect_data = RectData {
            top_left: Vertex::new(DIST_TO_SCREEN_EDGE, DIST_TO_SCREEN_EDGE + WORKSPACE_SELECTION_WINDOW_HEIGHT),
            width: screen_res.0.saturating_sub(DIST_TO_SCREEN_EDGE.saturating_mul(2)),
            height: screen_res
                .1
                .saturating_sub(DIST_TO_SCREEN_EDGE.saturating_mul(2).saturating_add(WORKSPACE_SELECTION_WINDOW_HEIGHT)),
        };

        let mut window = AppWindow::new(window_rect_data, DEFAULT_APP);
        let window_id = window.get_id();
        let content_rect = window.content_rect();

        // Build the complete app/window route before publishing the workspace.
        // Any startup failure therefore leaves the current layout unchanged
        // instead of reserving a window with no application behind it.
        let Some(input_queue_generation) = window.prepare_app_surface(content_rect) else {
            println!("WindowManager: failed to create input queue for window {}", window_id);
            return false;
        };
        let Some(child) = self.launch_gui_process(DEFAULT_APP, new_workspace_index, window_id, input_queue_generation) else {
            println!("WindowManager: failed to launch default app '{}' for window {}", DEFAULT_APP, window_id,);
            return false;
        };
        window.set_app_id(child.app_id());

        let workspace = Workspace::new_with_single_window((window_id, window), window_id);
        self.workspace_selection_window.register_workspace(workspace.get_id());
        self.workspaces.insert(new_workspace_index, workspace);
        self.switch_workspace(new_workspace_index);
        self.workspace_selection_window.mark_dirty();

        true
    }

    fn window_count(&self) -> usize {
        self.workspaces.iter().map(|workspace| workspace.windows.len()).sum()
    }

    fn remove_current_workspace(&mut self) {
        if self.workspaces.len() <= 1 {
            return;
        }

        self.remove_workspace(self.current_workspace, None);
    }

    fn remove_workspace(&mut self, workspace_index: usize, already_exited_app_id: Option<usize>) {
        if self.workspaces.len() <= 1 || workspace_index >= self.workspaces.len() {
            return;
        }

        let active_workspace_id = self.get_current_workspace().get_id();
        let removed_workspace_id = self.workspaces[workspace_index].get_id();
        self.workspace_selection_window.unregister_workspace(removed_workspace_id);

        let routes: Vec<(usize, usize)> = self.workspaces[workspace_index]
            .windows
            .iter()
            .filter_map(|(window_id, window)| window.app_id().map(|app_id| (app_id, *window_id)))
            .collect();
        for (app_id, window_id) in routes {
            if Some(app_id) != already_exited_app_id {
                let _ = close_process_surface(app_id, window_id);
            }
        }

        self.workspaces.remove(workspace_index);
        let workspace_ids: Vec<usize> = self.workspaces.iter().map(Workspace::get_id).collect();
        self.workspace_selection_window.renumber_workspaces(&workspace_ids);
        self.current_workspace = if active_workspace_id == removed_workspace_id {
            if workspace_index == 0 {
                self.workspaces.len() - 1
            } else {
                workspace_index - 1
            }
        } else {
            self.workspaces
                .iter()
                .position(|workspace| workspace.get_id() == active_workspace_id)
                .unwrap_or(0)
        };

        self.workspace_selection_window.mark_dirty();
        self.is_dirty = true;
    }

    fn switch_workspace(&mut self, workspace_index: usize) {
        if self.current_workspace != workspace_index {
            self.current_workspace = workspace_index;
            self.is_dirty = true;
            self.workspace_selection_window.mark_dirty();
        }
    }

    fn switch_prev_workspace(&mut self) {
        let prev_workspace = (self.current_workspace + self.workspaces.len() - 1) % self.workspaces.len();
        self.switch_workspace(prev_workspace);
    }

    fn switch_next_workspace(&mut self) {
        let next_workspace = (self.current_workspace + 1) % self.workspaces.len();
        self.switch_workspace(next_workspace);
    }

    fn get_current_workspace(&self) -> &Workspace {
        &self.workspaces[self.current_workspace]
    }

    fn get_current_workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.current_workspace]
    }

    fn draw(&mut self) {
        if self.command_line_window.is_active() {
            self.command_line_window.draw();
            return;
        }

        let is_dirty = self.is_dirty;

        if is_dirty {
            Drawer::full_clear_screen(false);
            self.workspace_selection_window.mark_dirty();
        }

        let focused_window_id = self.get_current_workspace().focused_window_id;
        let curr_ws = self.get_current_workspace_mut();

        for window in curr_ws.windows.values_mut().filter(|w| w.get_id() != focused_window_id) {
            window.draw(focused_window_id, is_dirty);
        }

        if let Some(window) = curr_ws.windows.get_mut(&focused_window_id) {
            window.draw(focused_window_id, is_dirty);
        }

        let current_workspace_id = curr_ws.get_id();
        self.workspace_selection_window.draw(Some(current_workspace_id));

        self.is_dirty = false;
    }

    fn is_app_name_valid(&self, app_string: &str) -> bool {
        matches!(
            app_string,
            "clock" | "submit_label" | "counter" | "slider" | "bitmap" | "calculator" | "radio" | "canvas" | "editor" | "layout"
        )
    }

    fn launch_gui_process(&self, app_name: &str, workspace_index: usize, window_id: usize, input_queue_generation: u64) -> Option<ChildProcess> {
        let screen = screen_resolution();
        let args: Vec<String> = alloc::vec![
            workspace_index.to_string(),
            window_id.to_string(),
            screen.0.to_string(),
            screen.1.to_string(),
            input_queue_generation.to_string(),
        ];
        let arg_refs = args.iter().map(String::as_str).collect();

        let child = run_gapp(app_name, arg_refs)?;
        if !init_process_event_channel(child.app_id(), window_id) {
            child.terminate();
            return None;
        }
        if !monitor_process_exit(child.app_id(), window_id, child.main_thread_id()) {
            child.terminate();
            release_process_state(child.app_id());
            return None;
        }
        Some(child)
    }

    fn fps(&self) -> i64 {
        let time = systime();
        let elapsed = time.checked_sub(&self.start_time).map(|elapsed| elapsed.num_milliseconds() / 1000).unwrap_or(0);

        if elapsed > 0 { self.frames / elapsed } else { 0 }
    }

    fn should_flush(&self) -> bool {
        let time = systime();
        let elapsed = time
            .checked_sub(&self.last_frame_time)
            .map(|elapsed| elapsed.num_milliseconds().max(0) as u32)
            .unwrap_or(0);

        elapsed >= FLUSHING_DELAY_MS
    }

    fn flush(&mut self) {
        if self.should_flush() {
            Drawer::flush();
            self.mouse_state.mark_dirty();
            self.last_frame_time = systime();
            self.frames = self
                .frames
                .checked_add(1)
                .or_else(|| {
                    self.start_time = self.last_frame_time;
                    Some(0)
                })
                .unwrap();
        }
    }

    fn enter_text_mode(&mut self) {
        Drawer::full_clear_screen(true);
        process::exit();
    }
}

#[cfg(feature = "with_runtime")]
#[no_mangle]
fn main() {
    use terminal::init_logger;

    init_logger();
    set_screen_resolution(Drawer::get_graphic_resolution());

    let mut window_manager = WindowManager::new();

    window_manager.init();
    window_manager.run();
}
