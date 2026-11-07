use alloc::{boxed::Box, rc::Rc};
use spin::RwLock;

use crate::{drawer::component_tree::ComponentTree, runtime::registry};

use super::ComponentFrameHandle;

pub fn current_component_frame() -> Result<ComponentFrameHandle, &'static str> {
    registry::with_api(|api| api.current_component_frame())
}

pub fn register_current_loop_iter(fun: Box<dyn Fn() -> bool>) -> Result<(), &'static str> {
    registry::with_api(|api| api.register_current_loop_iter(fun))
}

pub fn component_frame_for_window(window_id: usize) -> Option<Rc<RwLock<ComponentTree>>> {
    registry::with_api(|api| api.component_frame_for_window(window_id))
}
