use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use alloc::{boxed::Box, rc::Rc};
use drawer::{rect_data::RectData, vertex::Vertex};
use spin::{Mutex, RwLock};

use crate::{
    bus::frame_cache::LOGICAL_FRAME_SIZE,
    component::{Component, assistant::signal::ComponentRef},
    drawer::component_tree::ComponentTree,
};

/// Logical screen resolution, used by apps for describing component locations.
pub const LOG_SCREEN: (u32, u32) = LOGICAL_FRAME_SIZE;

static DEFER_SURFACE_UPDATES: AtomicBool = AtomicBool::new(false);
static NEXT_COMPONENT_ID: AtomicUsize = AtomicUsize::new(1);
static SCREEN_RESOLUTION: Mutex<(u32, u32)> = Mutex::new((0, 0));

pub fn next_component_id() -> usize {
    NEXT_COMPONENT_ID.fetch_add(1, Ordering::SeqCst)
}

pub fn set_screen_resolution(resolution: (u32, u32)) {
    *SCREEN_RESOLUTION.lock() = resolution;
}

pub fn screen_resolution() -> (u32, u32) {
    *SCREEN_RESOLUTION.lock()
}

/// Cloneable app-side handle to a GUI surface.
///
/// Apps create components directly, scale logical coordinates with this handle,
/// then attach the ComponentRef to the surface. Component creation is no
/// longer represented as a command enum.
#[derive(Clone)]
pub struct ComponentFrameHandle {
    pub app_id: usize,
    pub workspace_index: usize,
    pub window_id: usize,
    pub abs_pos: RectData,
    pub component_frame: Rc<RwLock<ComponentTree>>,
    pub ratios: (f64, f64),
}

impl ComponentFrameHandle {
    pub fn root_container(&self) -> ComponentRef {
        self.component_frame.read().root_container()
    }

    pub fn add(&self, parent: Option<ComponentRef>, component: ComponentRef) -> Result<ComponentRef, &str> {
        let parent = parent.unwrap_or_else(|| self.root_container());
        let mut frame = self.component_frame.write();
        if DEFER_SURFACE_UPDATES.load(Ordering::Acquire) {
            frame
                .insert_component_deferred(component.clone(), parent)
                .map_err(|_| "Failed to attach component")?;
        } else {
            frame
                .insert_component(component.clone(), parent)
                .map_err(|_| "Failed to update surface frame")?;
        }

        Ok(component)
    }

    /// Attach a component without immediately re-rendering the whole surface.
    ///
    /// Use this while building a larger component tree, then call
    /// update_frame() once at the end. This avoids N full FrameCache writes
    /// during app startup.
    pub fn add_deferred(&self, parent: Option<ComponentRef>, component: ComponentRef) -> Result<ComponentRef, &str> {
        let parent = parent.unwrap_or_else(|| self.root_container());

        self.component_frame
            .write()
            .insert_component_deferred(component.clone(), parent)
            .map_err(|_| "Failed to attach component")?;

        Ok(component)
    }

    pub fn update_frame(&self) -> Result<bool, &str> {
        self.component_frame.write().update_frame().map_err(|_| "Failed to update component frame")
    }

    pub fn mark_dirty(&self) {
        self.component_frame.write().mark_dirty();
    }

    pub fn scale_rect(&self, log_rect_data: RectData) -> Result<RectData, &str> {
        self.validate_log_pos(log_rect_data.top_left)?;
        Ok(scale_rect_data_with_ratios(self.ratios, log_rect_data))
    }

    pub fn scale_vertex(&self, log_pos: Vertex) -> Result<Vertex, &str> {
        self.validate_log_pos(log_pos)?;
        Ok(scale_vertex_with_ratios(self.ratios, log_pos))
    }

    pub fn scale_radius(&self, radius: u32) -> u32 {
        scale_radius_with_ratios(self.ratios, radius)
    }

    fn validate_log_pos(&self, log_pos: Vertex) -> Result<(), &str> {
        if log_pos.x > LOG_SCREEN.0 || log_pos.y > LOG_SCREEN.1 {
            return Err("Logical position-coordinates don't meet size-constraints");
        }

        Ok(())
    }
}

pub(crate) fn defer_surface_updates(defer: bool) {
    DEFER_SURFACE_UPDATES.store(defer, Ordering::Release);
}

/// Convert a concrete widget/container into a shared component reference.
pub fn component_ref<C: Component + 'static>(component: C) -> ComponentRef {
    use crate::component::assistant::signal::ComponentRefExt;
    ComponentRef::from_component(Box::new(component))
}

pub fn scale_rect_data_with_ratios(ratios: (f64, f64), log_rect_data: RectData) -> RectData {
    let new_pos = Vertex::new(
        (f64::from(log_rect_data.top_left.x) * ratios.0) as u32,
        (f64::from(log_rect_data.top_left.y) * ratios.1) as u32,
    );

    let new_width = f64::from(log_rect_data.width) * ratios.0;
    let new_height = f64::from(log_rect_data.height) * ratios.1;

    RectData {
        top_left: new_pos,
        width: new_width as u32,
        height: new_height as u32,
    }
}

pub fn scale_vertex_with_ratios(ratios: (f64, f64), log_pos: Vertex) -> Vertex {
    Vertex::new((f64::from(log_pos.x) * ratios.0) as u32, (f64::from(log_pos.y) * ratios.1) as u32)
}

pub fn scale_radius_with_ratios(ratios: (f64, f64), radius: u32) -> u32 {
    (f64::from(radius) * ratios.0.min(ratios.1)) as u32
}
