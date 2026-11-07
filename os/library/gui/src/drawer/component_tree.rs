use alloc::{boxed::Box, rc::Rc, vec::Vec};
use drawer::{rect_data::RectData, vertex::Vertex};
use syscall::return_vals::Errno;
use terminal::DecodedKey;

use crate::{
    bus::{
        frame_cache::{FrameCache, FrameType},
        input_queue::{DirectInputEvent, InputQueueReader},
    },
    component::{
        assistant::{
            mouse_state::MouseEvent,
            signal::{ComponentRef, ComponentRefExt},
        },
        container::{Container, basic::BasicContainer, styling::ContainerStylingBuilder},
    },
    drawer::draw_component_frame,
};

pub enum ComponentInteraction {
    Keyboard(DecodedKey),
    Mouse(MouseEvent),
}

/// App-owned GUI surface.
///
/// Window Manager owns only `AppWindow` chrome and composites this surface's
/// rendered `ComponentFrame` from `FrameCache`.
pub struct ComponentTree {
    app_id: usize,
    window_id: usize,
    rect: RectData,
    root_container: ComponentRef,
    focused_component: Option<ComponentRef>,
    keyboard_captured: bool,
    frame_cache: Option<FrameCache>,
    frame_size: (u32, u32),
    dirty: bool,
    input_queue: Option<InputQueueReader>,
    input_queue_generation: u64,
    input_events: Vec<DirectInputEvent>,
    input_queue_retry_loops: u8,
}

impl ComponentTree {
    pub fn new(app_id: usize, window_id: usize, input_queue_generation: u64, rect: RectData) -> Self {
        let mut root_container = BasicContainer::new(
            RectData {
                top_left: Vertex::zero(),
                width: 0,
                height: 0,
            },
            None,
            Some(ContainerStylingBuilder::new().show_border(false).show_background(true).child_padding(0).build()),
        );

        root_container.move_to(rect);

        Self {
            app_id,
            window_id,
            rect,
            root_container: ComponentRef::from_component(Box::new(root_container)),
            focused_component: None,
            keyboard_captured: false,
            frame_cache: None,
            frame_size: (0, 0),
            dirty: true,
            input_queue: InputQueueReader::open(window_id, input_queue_generation).ok(),
            input_queue_generation,
            input_events: Vec::new(),
            input_queue_retry_loops: 0,
        }
    }

    pub fn app_id(&self) -> usize {
        self.app_id
    }

    pub fn window_id(&self) -> usize {
        self.window_id
    }

    pub fn rect(&self) -> RectData {
        self.rect
    }

    pub fn root_container(&self) -> ComponentRef {
        self.root_container.clone()
    }

    pub fn resize(&mut self, rect: RectData) -> Result<(), Errno> {
        self.apply_rect(rect);
        self.update_frame().map(|_| ())
    }

    fn apply_rect(&mut self, rect: RectData) {
        if self.rect == rect {
            return;
        }

        self.rect = rect;
        self.root_container
            .write()
            .as_container_mut()
            .expect("ComponentFrame root must be a container")
            .move_to(rect);

        self.mark_dirty();
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.root_container.write().mark_dirty();
    }

    pub fn insert_component(&mut self, component: ComponentRef, parent: ComponentRef) -> Result<(), Errno> {
        self.insert_component_deferred(component, parent)?;
        self.update_frame().map(|_| ())
    }

    pub fn insert_component_deferred(&mut self, component: ComponentRef, parent: ComponentRef) -> Result<(), Errno> {
        if Rc::ptr_eq(&component, &parent) {
            return Err(Errno::EINVAL);
        }

        parent.write().as_container_mut().expect("parent must be a container").add_child(component);

        self.mark_dirty();
        Ok(())
    }

    pub fn update_frame(&mut self) -> Result<bool, Errno> {
        let tree_dirty = self.root_container.read().is_dirty();
        if !self.dirty && !tree_dirty {
            return Ok(false);
        }

        if self.rect.width == 0 || self.rect.height == 0 {
            return Ok(false);
        }

        self.ensure_frame_cache();

        let focused_id = self.focused_component.as_ref().map(|component| component.read().get_id());

        // `render_component_tree_to_bitmap` starts from a fresh bitmap. For a
        // FrameCache entry we must therefore render a complete frame, not only
        // the dirty leaves, otherwise the untouched parts of the cache become
        // transparent/undefined after every partial widget update.
        self.root_container.write().mark_dirty();

        let bitmap = draw_component_frame(self.root_container.clone(), self.rect, focused_id);

        if let Some(cache) = &self.frame_cache {
            cache.write_bitmap(&bitmap)?;
        }

        self.dirty = false;

        Ok(true)
    }

    pub fn handle_interaction(&mut self, interaction: ComponentInteraction) -> bool {
        let handled = match interaction {
            ComponentInteraction::Keyboard(keyboard_press) => self.interact_with_focused_component_keyboard(keyboard_press),
            ComponentInteraction::Mouse(mouse_event) => {
                // The focused styling is also the component hover state. Every
                // mouse packet carries the current absolute position, so focus
                // follows the pointer even when no button is pressed. The
                // shared input queue preserves button-transition order.
                let focus_changed = self.focus_component_at(mouse_event.position);
                let interacted = self.interact_with_focused_component_mouse(mouse_event);
                focus_changed || interacted
            }
        };

        if handled {
            self.publish_keyboard_capture();
        }

        handled
    }

    /// Consume WM-routed input directly from shared memory in the application
    /// process. Component callbacks and text editing therefore run in libgui;
    /// EventPipe remains reserved for low-frequency process control events.
    pub fn process_pending_input(&mut self) {
        if self.input_queue.is_none() {
            if self.input_queue_retry_loops > 0 {
                self.input_queue_retry_loops -= 1;
                return;
            }
            match InputQueueReader::open(self.window_id, self.input_queue_generation) {
                Ok(queue) => self.input_queue = Some(queue),
                Err(_) => {
                    self.input_queue_retry_loops = 16;
                    return;
                }
            }
        }

        let mut events = core::mem::take(&mut self.input_events);
        if let Some(queue) = &self.input_queue {
            queue.drain_into(&mut events);
        } else {
            events.clear();
        }
        for event in events.drain(..) {
            match event {
                DirectInputEvent::Keyboard(key) => {
                    self.handle_interaction(ComponentInteraction::Keyboard(key));
                }
                DirectInputEvent::Mouse(mouse) => {
                    self.handle_interaction(ComponentInteraction::Mouse(mouse));
                }
                DirectInputEvent::FocusNext => self.focus_next_component(),
                DirectInputEvent::FocusPrev => self.focus_prev_component(),
                DirectInputEvent::Unfocus => self.unfocus(),
                DirectInputEvent::WindowRect(rect) => self.apply_rect(rect),
            }
        }
        self.input_events = events;
    }

    pub fn focus_next_component(&mut self) {
        if !self.can_unfocus() {
            return;
        }

        let next_component = self
            .root_container
            .write()
            .as_container_mut()
            .expect("ComponentFrame root must be a container")
            .focus_next_child();

        self.focus_component(next_component);
    }

    pub fn focus_prev_component(&mut self) {
        if !self.can_unfocus() {
            return;
        }

        let prev_component = self
            .root_container
            .write()
            .as_container_mut()
            .expect("ComponentFrame root must be a container")
            .focus_prev_child();

        self.focus_component(prev_component);
    }

    pub fn focus_component_at(&mut self, pos: Vertex) -> bool {
        if !self.can_unfocus() {
            return false;
        }

        let next_component = self
            .root_container
            .write()
            .as_container_mut()
            .expect("ComponentFrame root must be a container")
            .focus_child_at(pos);

        self.focus_component(next_component)
    }

    pub fn can_unfocus(&self) -> bool {
        if let Some(focused) = &self.focused_component {
            if let Some(focusable) = focused.read().as_focusable() {
                return focusable.can_unfocus();
            }
        }

        true
    }

    pub fn unfocus(&mut self) {
        if let Some(focused) = &self.focused_component {
            if let Some(focusable) = focused.write().as_focusable_mut() {
                focusable.unfocus();
            }
        }

        self.focused_component = None;
        self.mark_dirty();
        self.publish_keyboard_capture();
    }

    fn focus_component(&mut self, component: Option<ComponentRef>) -> bool {
        let focused_id = self.focused_component.as_ref().map(|component| component.read().get_id());
        let next_id = component.as_ref().map(|component| component.read().get_id());

        if focused_id == next_id {
            return false;
        }

        if let Some(current_component) = &self.focused_component {
            if let Some(focusable) = current_component.write().as_focusable_mut() {
                if !focusable.can_unfocus() {
                    return false;
                }

                focusable.unfocus();
            }
        }

        self.focused_component = None;

        if let Some(component) = component {
            if let Some(focusable) = component.write().as_focusable_mut() {
                focusable.focus();
                self.focused_component = Some(component.clone());
            }
        }

        self.mark_dirty();
        self.publish_keyboard_capture();
        true
    }

    fn publish_keyboard_capture(&mut self) {
        let captured = self
            .focused_component
            .as_ref()
            .and_then(|component| component.read().as_interactable().map(|interactable| interactable.captures_keyboard()))
            .unwrap_or(false);

        if captured == self.keyboard_captured {
            return;
        }
        self.keyboard_captured = captured;

        if let Some(queue) = &self.input_queue {
            queue.set_keyboard_captured(captured);
        }
    }

    fn interact_with_focused_component_keyboard(&mut self, keyboard_press: DecodedKey) -> bool {
        if let Some(focused_component) = &self.focused_component {
            let callback = if let Some(interactable) = focused_component.write().as_interactable_mut() {
                interactable.consume_keyboard_press(keyboard_press)
            } else {
                None
            };

            if let Some(callback) = callback {
                callback();
                self.mark_dirty();
                return true;
            }
        }

        false
    }

    fn interact_with_focused_component_mouse(&mut self, mouse_event: MouseEvent) -> bool {
        if let Some(focused_component) = &self.focused_component {
            let callback = if let Some(interactable) = focused_component.write().as_interactable_mut() {
                interactable.consume_mouse_event(&mouse_event)
            } else {
                None
            };

            if let Some(callback) = callback {
                callback();
                self.mark_dirty();
                return true;
            }
        }

        false
    }

    fn ensure_frame_cache(&mut self) {
        let size = (self.rect.width, self.rect.height);

        if self.frame_cache.is_none() {
            self.frame_cache = Some(FrameCache::new(
                self.app_id,
                self.window_id,
                self.rect.width,
                self.rect.height,
                FrameType::ComponentFrame,
            ));
            self.frame_size = size;
            return;
        }

        if self.frame_size != size {
            let cache = self.frame_cache.take().expect("ComponentFrame cache must exist before resize");
            self.frame_cache = Some(cache.recreate(self.rect.width, self.rect.height));
            self.frame_size = size;
        }
    }
}
