use alloc::{boxed::Box, string::String};
use drawer::{drawer::Drawer, rect_data::RectData, vertex::Vertex};
use graphic::{
    bitmap::Bitmap,
    color::{self, Color, INVISIBLE},
    lfb::{DEFAULT_CHAR_HEIGHT, DEFAULT_CHAR_WIDTH},
};

use gui::{
    api::next_component_id,
    api::{WindowManagerMessage, send_window_manager_message},
    bus::{
        frame_cache::{FrameCache, FrameType},
        input_queue::InputQueueWriter,
    },
    component::{
        assistant::{
            config::{DEFAULT_FG_COLOR, FOCUSED_BG_COLOR},
            mouse_state::MouseEvent,
            signal::{ComponentRef, ComponentRefExt, Signal},
        },
        container::{
            Container,
            basic::BasicContainer,
            layout::{AlignmentMode, ContainerLayoutBuilder, HorizontalDirection, StretchMode},
            styling::ContainerStylingBuilder,
        },
        styling::ComponentStylingBuilder,
        widget::button::Button,
    },
    drawer::{bitmap_drawer::BitmapDrawer, draw_component_frame},
};
use terminal::DecodedKey;

pub const ACTION_BAR_HEIGHT: u32 = 40;
pub const FOCUSED_INDICATOR_COLOR: Color = FOCUSED_BG_COLOR;
pub const FOCUSED_INDICATOR_LENGTH: u32 = 24;

/// Window Manager side window chrome.
///
/// App content is no longer stored here. `ComponentFrame` owns the component tree,
/// renders it into `FrameCache::ComponentFrame`, and this window only composites
/// that cached bitmap into `content_rect()`.
pub struct AppWindow {
    id: usize,
    app_id: Option<usize>,
    app_name: String,
    title: Option<String>,
    pub rect_data: RectData,
    is_dirty: bool,
    is_dragging: bool,

    action_container: ComponentRef,
    close_button: ComponentRef,
    next_button: ComponentRef,
    prev_button: ComponentRef,
    focused_chrome_component: Option<ComponentRef>,

    component_frame: Option<Bitmap>,
    /// Reused import buffer for the raw shared-memory frame. Keeping this
    /// separate lets the compositor retain the last accepted frame if a
    /// sequence-lock read races with the application.
    component_frame_scratch: Option<Bitmap>,
    component_frame_cache: Option<FrameCache>,
    component_frame_revision: u64,
    component_cache_retry_loops: u8,
    app_frame_dirty: bool,
    input_queue: Option<InputQueueWriter>,
}

impl AppWindow {
    pub fn new(rect_data: RectData, app_name: &str) -> Self {
        let id = next_component_id();

        let mut action_container = Box::new(BasicContainer::new(
            RectData {
                top_left: Vertex::zero(),
                width: 0,
                height: 0,
            },
            Some(
                ContainerLayoutBuilder::new()
                    .alignment(AlignmentMode::Horizontal(HorizontalDirection::Right))
                    .stretch(StretchMode::Fill)
                    .build(),
            ),
            Some(ContainerStylingBuilder::new().show_border(true).show_background(true).build()),
        ));

        let button_rect = RectData {
            top_left: Vertex::zero(),
            width: 30,
            height: 0,
        };

        let close_button = Button::new(
            button_rect,
            button_rect,
            Some(Signal::new(String::from("X"))),
            1,
            Some(Box::new(|| {
                let _ = send_window_manager_message(WindowManagerMessage::CloseCurrentWindow);
            })),
            Some(
                ComponentStylingBuilder::new()
                    .maintain_aspect_ratio(true)
                    .focused_border_color(color::RED)
                    .focused_background_color(color::RED.dim())
                    .build(),
            ),
        );

        let next_button = Button::new(
            button_rect,
            button_rect,
            Some(Signal::new(String::from(">"))),
            1,
            Some(Box::new(|| {
                let _ = send_window_manager_message(WindowManagerMessage::MoveCurrentWindowForward);
            })),
            Some(ComponentStylingBuilder::new().maintain_aspect_ratio(true).build()),
        );

        let prev_button = Button::new(
            button_rect,
            button_rect,
            Some(Signal::new(String::from("<"))),
            1,
            Some(Box::new(|| {
                let _ = send_window_manager_message(WindowManagerMessage::MoveCurrentWindowBackward);
            })),
            Some(ComponentStylingBuilder::new().maintain_aspect_ratio(true).build()),
        );

        action_container.add_child(close_button.clone());
        action_container.add_child(next_button.clone());
        action_container.add_child(prev_button.clone());
        action_container.move_to(Self::chrome_rect_for(rect_data));

        Self {
            id,
            app_id: None,
            app_name: String::from(app_name),
            title: None,
            rect_data,
            is_dirty: true,
            is_dragging: false,
            action_container: ComponentRef::from_component(action_container),
            close_button,
            next_button,
            prev_button,
            focused_chrome_component: None,
            component_frame: None,
            component_frame_scratch: None,
            component_frame_cache: None,
            component_frame_revision: 0,
            component_cache_retry_loops: 0,
            app_frame_dirty: true,
            input_queue: None,
        }
    }

    pub fn get_id(&self) -> usize {
        self.id
    }

    pub fn app_id(&self) -> Option<usize> {
        self.app_id
    }

    pub fn set_app_id(&mut self, app_id: usize) {
        if self.input_queue.is_none() {
            self.prepare_app_surface(self.content_rect());
        }
        self.app_id = Some(app_id);
        self.component_frame = None;
        self.component_frame_scratch = None;
        self.component_frame_cache = None;
        self.component_frame_revision = 0;
        self.component_cache_retry_loops = 0;
        self.app_frame_dirty = true;
    }

    pub fn set_app_name(&mut self, app_name: &str) {
        self.app_name = String::from(app_name);
        self.title = None;
        self.is_dirty = true;
    }

    pub fn set_title(&mut self, title: Option<String>) {
        self.title = title.filter(|title| !title.is_empty());
        self.is_dirty = true;
    }

    pub fn disconnect_app(&mut self) {
        self.app_id = None;
        self.component_frame = None;
        self.component_frame_scratch = None;
        self.component_frame_cache = None;
        self.component_frame_revision = 0;
        self.component_cache_retry_loops = 0;
        self.is_dirty = true;
        self.app_frame_dirty = true;
        if let Some(queue) = &mut self.input_queue {
            queue.disconnect();
        }
    }

    /// Recreate the SPSC route and publish geometry before launching a new app.
    /// A previous app retains only its unlinked generation; the replacement can
    /// consume this generation as soon as libgui attaches to shared memory.
    pub fn prepare_app_surface(&mut self, rect: RectData) -> Option<u64> {
        self.input_queue = match self.input_queue.take() {
            Some(queue) => queue.recreate().or_else(|_| InputQueueWriter::create(self.id)).ok(),
            None => InputQueueWriter::create(self.id).ok(),
        };
        let queue = self.input_queue.as_mut()?;
        queue.push_window_rect(rect);
        queue.flush();
        Some(queue.generation())
    }

    pub fn send_keyboard_input(&mut self, key: DecodedKey) {
        if self.app_id.is_some() {
            if let Some(queue) = &mut self.input_queue {
                queue.push_keyboard(key);
            }
        }
    }

    pub fn send_mouse_input(&mut self, event: &MouseEvent) {
        if self.app_id.is_some() {
            if let Some(queue) = &mut self.input_queue {
                queue.push_mouse(event);
            }
        }
    }

    pub fn focus_next_app_component(&mut self) {
        if self.app_id.is_some() {
            if let Some(queue) = &mut self.input_queue {
                queue.push_focus_next();
            }
        }
    }

    pub fn focus_prev_app_component(&mut self) {
        if self.app_id.is_some() {
            if let Some(queue) = &mut self.input_queue {
                queue.push_focus_prev();
            }
        }
    }

    pub fn unfocus_app_surface(&mut self) {
        if self.app_id.is_some() {
            if let Some(queue) = &mut self.input_queue {
                queue.push_unfocus();
            }
        }
    }

    pub fn resize_app_surface(&mut self, rect: RectData) {
        if self.app_id.is_some() {
            if let Some(queue) = &mut self.input_queue {
                queue.push_window_rect(rect);
            }
        }
    }

    pub fn mark_window_dirty(&mut self) {
        self.is_dirty = true;
    }

    pub fn mark_app_frame_dirty(&mut self) {
        self.app_frame_dirty = true;
    }

    /// Poll the shared FrameCache header and import the bitmap only when the
    /// application has published a new complete revision.
    pub fn poll_component_frame(&mut self) -> bool {
        if let Some(queue) = &mut self.input_queue {
            queue.flush();
        }
        let Some(app_id) = self.app_id else {
            return false;
        };

        let content = self.content_rect();
        if content.width == 0 || content.height == 0 {
            return false;
        }

        if self.component_frame_cache.is_none() {
            if self.component_cache_retry_loops > 0 {
                self.component_cache_retry_loops -= 1;
                return false;
            }
            let Ok(cache) = FrameCache::try_get_instance(app_id, self.id, content.width, content.height, FrameType::ComponentFrame) else {
                // During startup/resize the name may not exist yet or may
                // still refer to the old dimensions. Avoid a tight loop of
                // attach/detach calls while the application catches up.
                self.component_cache_retry_loops = 16;
                return false;
            };
            self.component_frame_cache = Some(cache);
            self.component_frame_revision = 0;
            self.component_cache_retry_loops = 0;
        }

        let cache = self.component_frame_cache.as_ref().expect("component frame cache just initialized");
        let Ok(Some(revision)) = cache.completed_revision() else {
            return false;
        };
        if revision == self.component_frame_revision {
            return false;
        }

        let mut source = self.component_frame_scratch.take().unwrap_or(Bitmap {
            width: 0,
            height: 0,
            data: alloc::vec::Vec::new(),
        });
        let Ok(Some(imported_revision)) = cache.read_bitmap_snapshot_into(&mut source) else {
            self.component_frame_scratch = Some(source);
            return false;
        };

        // Scaling happens only after the sequence-lock copy. If no scaling is
        // needed, the previous accepted frame becomes the next scratch buffer.
        if source.width == content.width && source.height == content.height {
            self.component_frame_scratch = self.component_frame.replace(source);
        } else {
            let mut frame = self.component_frame.take().unwrap_or(Bitmap {
                width: 0,
                height: 0,
                data: alloc::vec::Vec::new(),
            });
            if !Self::scale_frame_into(&source, &mut frame, content.width, content.height) {
                self.component_frame_scratch = Some(source);
                self.component_frame = Some(frame);
                return false;
            }
            self.component_frame_scratch = Some(source);
            self.component_frame = Some(frame);
        }

        self.component_frame_revision = imported_revision;
        self.app_frame_dirty = true;
        true
    }

    fn scale_frame_into(source: &Bitmap, target: &mut Bitmap, width: u32, height: u32) -> bool {
        if source.width == 0 || source.height == 0 || width == 0 || height == 0 {
            return false;
        }
        let Some(pixel_count) = (width as usize).checked_mul(height as usize) else {
            return false;
        };

        target.width = width;
        target.height = height;
        target.data.resize(pixel_count, INVISIBLE);

        for y in 0..height {
            let source_y = y * source.height / height;
            for x in 0..width {
                let source_x = x * source.width / width;
                target.data[(y * width + x) as usize] = source.data[(source_y * source.width + source_x) as usize];
            }
        }

        true
    }

    pub fn captures_keyboard(&self) -> bool {
        self.input_queue.as_ref().is_some_and(InputQueueWriter::keyboard_captured)
    }

    pub fn needs_redraw(&self) -> bool {
        self.is_dirty || self.app_frame_dirty || self.action_container.read().is_dirty()
    }

    pub fn content_rect(&self) -> RectData {
        RectData {
            top_left: self.rect_data.top_left.add(0, ACTION_BAR_HEIGHT),
            width: self.rect_data.width,
            height: self.rect_data.height.saturating_sub(ACTION_BAR_HEIGHT),
        }
    }

    pub fn handle_chrome_mouse_event(&mut self, mouse_event: &MouseEvent) -> bool {
        let chrome_rect = self.action_container.read().get_abs_rect_data();
        if !chrome_rect.contains_vertex(&mouse_event.position) {
            return false;
        }

        let new_component = self
            .action_container
            .write()
            .as_container_mut()
            .expect("action_container must be a container")
            .focus_child_at(mouse_event.position);

        self.focus_chrome_component(new_component);

        if let Some(component) = &self.focused_chrome_component {
            let callback = if let Some(interactable) = component.write().as_interactable_mut() {
                interactable.consume_mouse_event(mouse_event)
            } else {
                None
            };

            if let Some(callback) = callback {
                callback();
                self.is_dirty = true;
                return true;
            }
        }

        true
    }

    pub fn can_unfocus(&self) -> bool {
        if let Some(focused) = &self.focused_chrome_component {
            if let Some(focusable) = focused.read().as_focusable() {
                return focusable.can_unfocus();
            }
        }

        true
    }

    pub fn unfocus(&mut self) {
        if let Some(focused) = &self.focused_chrome_component {
            if let Some(focusable) = focused.write().as_focusable_mut() {
                focusable.unfocus();
            }
        }

        self.focused_chrome_component = None;
        self.is_dirty = true;
    }

    pub fn rescale_window_in_place(&mut self, new_abs_rect: RectData) {
        self.rect_data = new_abs_rect;
        self.action_container
            .write()
            .as_container_mut()
            .expect("action_container must be a container")
            .move_to(Self::chrome_rect_for(new_abs_rect));

        self.mark_window_dirty();
        self.mark_app_frame_dirty();
        self.component_frame = None;
        self.component_frame_scratch = None;
        self.component_frame_cache = None;
        self.component_frame_revision = 0;
        self.component_cache_retry_loops = 0;
    }

    pub fn rescale_window_after_move(&mut self, new_abs_rect: RectData) {
        self.rescale_window_in_place(new_abs_rect);
    }

    pub fn merge(&mut self, other_window: &mut AppWindow) {
        let new_top_left = Vertex::new(
            self.rect_data.top_left.x.min(other_window.rect_data.top_left.x),
            self.rect_data.top_left.y.min(other_window.rect_data.top_left.y),
        );
        let right = self
            .rect_data
            .top_left
            .x
            .saturating_add(self.rect_data.width)
            .max(other_window.rect_data.top_left.x.saturating_add(other_window.rect_data.width));
        let bottom = self
            .rect_data
            .top_left
            .y
            .saturating_add(self.rect_data.height)
            .max(other_window.rect_data.top_left.y.saturating_add(other_window.rect_data.height));

        self.rescale_window_in_place(RectData {
            top_left: new_top_left,
            width: right.saturating_sub(new_top_left.x),
            height: bottom.saturating_sub(new_top_left.y),
        });

        other_window.mark_window_dirty();
    }

    pub fn draw(&mut self, focused_window_id: usize, full: bool) {
        if full {
            self.is_dirty = true;
            self.app_frame_dirty = true;
        }

        if !self.needs_redraw() {
            return;
        }

        let redraw_window_chrome = self.is_dirty;
        let redraw_app_frame = self.is_dirty || self.app_frame_dirty;
        let redraw_action_bar = self.is_dirty || self.action_container.read().is_dirty();
        let is_focused = self.id == focused_window_id;

        if redraw_window_chrome {
            Drawer::partial_clear_screen(self.rect_data);
            self.action_container.write().mark_dirty();
        }

        if redraw_app_frame {
            self.draw_cached_app_frame();
        }

        if redraw_action_bar || redraw_window_chrome {
            self.draw_action_bar();
        }

        if redraw_window_chrome {
            // Drawer rectangles include their end coordinate. Keep the border
            // inside this window so it cannot overwrite an adjacent tile.
            let border_rect = RectData {
                width: self.rect_data.width.saturating_sub(1),
                height: self.rect_data.height.saturating_sub(1),
                ..self.rect_data
            };

            if is_focused {
                Drawer::draw_rectangle(border_rect, FOCUSED_BG_COLOR);
            } else {
                Drawer::draw_rectangle(border_rect, DEFAULT_FG_COLOR);
            }
        }

        self.is_dirty = false;
        self.app_frame_dirty = false;
    }

    fn draw_action_bar(&mut self) {
        let chrome_rect = self.action_container.read().get_abs_rect_data();
        let focused_id = self.focused_chrome_component.as_ref().map(|component| component.read().get_id());

        let mut bitmap = draw_component_frame(self.action_container.clone(), chrome_rect, focused_id);

        let title = self.title.as_ref().unwrap_or(&self.app_name);
        let button_area_width = 3u32.saturating_mul(30);
        let horizontal_padding = 8u32;
        let available_width = chrome_rect
            .width
            .saturating_sub(button_area_width)
            .saturating_sub(horizontal_padding.saturating_mul(2));
        let max_chars = (available_width / DEFAULT_CHAR_WIDTH) as usize;
        if max_chars > 0 {
            let visible_title = title.chars().take(max_chars).collect::<String>();
            let title_pos = chrome_rect
                .top_left
                .add(horizontal_padding, chrome_rect.height.saturating_sub(DEFAULT_CHAR_HEIGHT) / 2);
            BitmapDrawer::new_with_origin(&mut bitmap, chrome_rect.top_left).draw_string(visible_title, title_pos, DEFAULT_FG_COLOR, None, (1, 1));
        }

        Drawer::draw_bitmap(chrome_rect.top_left, &bitmap);
    }

    fn draw_cached_app_frame(&mut self) {
        let content_rect = self.content_rect();
        if let Some(frame) = &self.component_frame {
            Drawer::draw_bitmap(content_rect.top_left, frame);
        }
    }

    fn focus_chrome_component(&mut self, component: Option<ComponentRef>) {
        let focused_id = self.focused_chrome_component.as_ref().map(|component| component.read().get_id());
        let next_id = component.as_ref().map(|component| component.read().get_id());

        if focused_id == next_id {
            return;
        }

        if let Some(current_component) = &self.focused_chrome_component {
            if let Some(focusable) = current_component.write().as_focusable_mut() {
                if !focusable.can_unfocus() {
                    return;
                }

                focusable.unfocus();
            }
        }

        self.focused_chrome_component = None;

        if let Some(component) = component {
            if let Some(focusable) = component.write().as_focusable_mut() {
                focusable.focus();
                self.focused_chrome_component = Some(component.clone());
            }
        }

        self.is_dirty = true;
    }

    fn chrome_rect_for(rect_data: RectData) -> RectData {
        RectData {
            top_left: rect_data.top_left,
            width: rect_data.width,
            height: ACTION_BAR_HEIGHT,
        }
    }
}
