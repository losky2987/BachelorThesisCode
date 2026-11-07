#![allow(static_mut_refs)]

use core::fmt::Debug;

use alloc::{boxed::Box, rc::Rc, vec::Vec};
use concurrent::thread;
use drawer::rect_data::RectData;
use hashbrown::HashMap;
use spin::{Mutex, MutexGuard, RwLock, once::Once};

use crate::{
    api::{ComponentFrameHandle, LOG_SCREEN, screen_resolution},
    drawer::component_tree::ComponentTree,
};

/// All information saved for one app thread / GUI handle.
pub struct HandleData {
    pub app_id: usize,
    pub workspace_index: usize,
    pub window_id: usize,
    pub abs_pos: RectData,
    pub component_frame: Rc<RwLock<ComponentTree>>,
    /// Ratio from the app logical coordinate space to full-screen coordinates.
    /// Containers apply the window-to-screen transform during layout, so this
    /// must not include the window size or components would be scaled twice.
    pub ratios: (f64, f64),
}

#[derive(Clone, Copy, Debug)]
pub struct WindowData {
    pub workspace_index: usize,
    pub window_id: usize,
}

pub struct NewLoopIterFnData {
    pub window_data: WindowData,
    pub fun: Box<dyn Fn() -> bool>,
}

impl Debug for NewLoopIterFnData {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NewLoopIterFnData").field("window_data", &self.window_data).finish()
    }
}

/// Application-side GUI runtime registry.
///
/// It only stores surface registrations, loop callbacks, and WM messaging.
/// Component construction stays in the application code.
pub struct GuiApi {
    /// Keys are app thread IDs.
    pub(crate) handles: HashMap<usize, HandleData>,
    pub(crate) on_loop_iter_fns: Vec<NewLoopIterFnData>,
}

static mut API: Once<Mutex<GuiApi>> = Once::new();

pub fn init_api() {
    unsafe {
        API.call_once(|| Mutex::new(GuiApi::new()));
    }
}

pub fn get_api() -> MutexGuard<'static, GuiApi> {
    init_api();
    unsafe { API.get().expect("GuiApi not initialized").lock() }
}

pub fn with_api<R>(f: impl FnOnce(&mut GuiApi) -> R) -> R {
    let mut api = get_api();
    f(&mut api)
}

impl GuiApi {
    pub fn new() -> Self {
        Self {
            handles: HashMap::new(),
            on_loop_iter_fns: Vec::new(),
        }
    }

    pub fn register_surface(
        &mut self, thread_id: usize, app_id: usize, workspace_index: usize, window_id: usize, abs_pos: RectData, component_frame: Rc<RwLock<ComponentTree>>,
    ) {
        let screen = screen_resolution();
        let handle_data = HandleData {
            app_id,
            workspace_index,
            window_id,
            abs_pos,
            component_frame,
            ratios: (f64::from(screen.0) / f64::from(LOG_SCREEN.0), f64::from(screen.1) / f64::from(LOG_SCREEN.1)),
        };

        self.handles.insert(thread_id, handle_data);
    }

    pub(crate) fn update_surface_rect(&mut self, window_id: usize, rect: RectData) {
        if let Some(handle) = self.handles.values_mut().find(|handle| handle.window_id == window_id) {
            handle.abs_pos = rect;
        }
    }

    pub fn current_component_frame(&self) -> Result<ComponentFrameHandle, &'static str> {
        let thread_id = thread::current().map(|thread| thread.id()).ok_or("Current thread not found")?;

        self.component_frame_for_handle(thread_id)
    }

    pub fn component_frame_for_handle(&self, window_handle: usize) -> Result<ComponentFrameHandle, &'static str> {
        let handle = self.handles.get(&window_handle).ok_or("Provided handle not found")?;

        Ok(ComponentFrameHandle {
            app_id: handle.app_id,
            workspace_index: handle.workspace_index,
            window_id: handle.window_id,
            abs_pos: handle.abs_pos,
            component_frame: handle.component_frame.clone(),
            ratios: handle.ratios,
        })
    }

    pub fn component_frame_for_window(&self, window_id: usize) -> Option<Rc<RwLock<ComponentTree>>> {
        self.handles
            .values()
            .find(|handle| handle.window_id == window_id)
            .map(|handle| handle.component_frame.clone())
    }

    pub fn register_current_loop_iter(&mut self, fun: Box<dyn Fn() -> bool>) -> Result<(), &'static str> {
        let handle = self.current_component_frame()?;
        self.on_loop_iter_fns.push(NewLoopIterFnData {
            window_data: WindowData {
                workspace_index: handle.workspace_index,
                window_id: handle.window_id,
            },
            fun,
        });

        Ok(())
    }
}

impl Default for GuiApi {
    fn default() -> Self {
        Self::new()
    }
}
