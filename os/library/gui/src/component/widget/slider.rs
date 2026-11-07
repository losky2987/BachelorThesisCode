use alloc::{boxed::Box, rc::Rc};
use drawer::{rect_data::RectData, vertex::Vertex};
use graphic::lfb::DEFAULT_CHAR_HEIGHT;
use libm::round;
use terminal::DecodedKey;

use crate::{
    component::{
        Casts, Component, Disableable, Focusable, Hideable, Interactable,
        assistant::{
            mouse_state::{MouseEvent, ScrollDirection},
            signal::{ComponentRef, ComponentRefExt, Stateful},
        },
        container::Container,
        styling::ComponentStyling,
    },
    drawer::bitmap_drawer::BitmapDrawer,
    api::next_component_id,
};

const HANDLE_WIDTH: u32 = 10;

pub struct Slider {
    id: usize,
    value: Stateful<i32>,
    min: i32,
    max: i32,
    abs_rect_data: RectData,
    rel_rect_data: RectData,
    orig_rect_data: RectData,
    drawn_rect_data: RectData,
    on_change: Rc<Box<dyn Fn(i32) -> ()>>,
    steps: u32,
    is_dirty: bool,

    // hideable
    is_hidden: bool,
    // disableable
    is_disabled: bool,

    is_dragging: bool,

    styling: ComponentStyling,
}

impl Slider {
    pub fn new(
        rel_rect_data: RectData, orig_rect_data: RectData, on_change: Option<Box<dyn Fn(i32) -> ()>>, value: Stateful<i32>, min: i32, max: i32, steps: u32,
        styling: Option<ComponentStyling>,
    ) -> ComponentRef {
        let (min, max) = if min <= max { (min, max) } else { (max, min) };
        let steps = steps.max(1);
        let initial = value.get().clamp(min, max);
        if initial != value.get() {
            value.set(initial);
        }

        let slider = Box::new(Self {
            id: next_component_id(),
            abs_rect_data: RectData::zero(),
            rel_rect_data,
            orig_rect_data,
            drawn_rect_data: RectData::zero(),
            on_change: Rc::new(on_change.unwrap_or_else(|| Box::new(|_| {}))),
            steps,
            value: Stateful::clone(&value),
            min,
            max,
            is_dirty: true,
            is_disabled: false,
            is_hidden: false,
            is_dragging: false,
            styling: styling.unwrap_or_default(),
        });

        let component = ComponentRef::from_component(slider);
        value.register_component(ComponentRef::clone(&component));

        component
    }

    fn update_value(&mut self, new_value: i32) -> Option<Box<dyn FnOnce() -> ()>> {
        let new_value = new_value.clamp(self.min, self.max);
        if new_value == self.value.get() {
            return None;
        }

        let on_change = Rc::clone(&self.on_change);
        let value = Rc::clone(&self.value);

        Some(Box::new(move || {
            value.set(new_value);
            on_change(new_value);
        }))
    }
}

impl Component for Slider {
    fn draw(&mut self, focus_id: Option<usize>, drawer: &mut BitmapDrawer) {
        if !self.is_dirty {
            return;
        }

        if self.is_hidden {
            self.is_dirty = false;
            return;
        }

        let styling = &self.styling;
        let is_focused = focus_id == Some(self.id);

        let bg_color = if self.is_disabled {
            styling.disabled_background_color
        } else if is_focused {
            styling.focused_background_color
        } else {
            styling.background_color
        };

        let border_color = if is_focused {
            styling.focused_border_color
        } else if self.is_disabled {
            styling.disabled_border_color
        } else {
            styling.border_color
        };

        drawer.draw_filled_rectangle(self.abs_rect_data, bg_color, Some(border_color));

        self.drawn_rect_data = self.abs_rect_data;

        let value = self.value.get();
        let range = f64::from(self.max) - f64::from(self.min);
        let normalized_value = if range == 0.0 {
            0.0
        } else {
            ((f64::from(value.clamp(self.min, self.max)) - f64::from(self.min)) / range).clamp(0.0, 1.0)
        };
        let slider_position = ((self.abs_rect_data.width.saturating_sub(HANDLE_WIDTH) as f64) * normalized_value) as u32;

        let handle_rect = RectData {
            top_left: Vertex {
                x: self.abs_rect_data.top_left.x.saturating_add(slider_position),
                y: self.abs_rect_data.top_left.y,
            },
            width: HANDLE_WIDTH,
            height: self.abs_rect_data.height,
        };

        drawer.draw_filled_rectangle(handle_rect, styling.background_color, Some(styling.border_color));

        self.is_dirty = false;
    }

    fn rescale_to_container(&mut self, parent: &dyn Container) {
        let styling: &ComponentStyling = &self.styling;

        let min_dim = (HANDLE_WIDTH.saturating_mul(self.steps), DEFAULT_CHAR_HEIGHT);
        let max_dim = (self.orig_rect_data.width, self.orig_rect_data.height);

        self.abs_rect_data = parent.scale_to_container(self.rel_rect_data, min_dim, max_dim, styling.maintain_aspect_ratio);

        self.mark_dirty();
    }

    fn is_dirty(&self) -> bool {
        self.is_dirty
    }

    fn mark_dirty(&mut self) {
        self.is_dirty = true;
    }

    fn get_id(&self) -> usize {
        self.id
    }

    fn get_abs_rect_data(&self) -> RectData {
        self.abs_rect_data
    }

    fn get_drawn_rect_data(&self) -> RectData {
        self.drawn_rect_data
    }
}

impl Casts for Slider {
    fn as_disableable(&self) -> Option<&dyn Disableable> {
        Some(self)
    }

    fn as_hideable(&self) -> Option<&dyn Hideable> {
        Some(self)
    }

    fn as_focusable(&self) -> Option<&dyn Focusable> {
        Some(self)
    }

    fn as_focusable_mut(&mut self) -> Option<&mut dyn Focusable> {
        Some(self)
    }

    fn as_interactable(&self) -> Option<&dyn Interactable> {
        Some(self)
    }

    fn as_disableable_mut(&mut self) -> Option<&mut dyn Disableable> {
        Some(self)
    }

    fn as_hideable_mut(&mut self) -> Option<&mut dyn Hideable> {
        Some(self)
    }

    fn as_interactable_mut(&mut self) -> Option<&mut dyn Interactable> {
        Some(self)
    }
}

impl Disableable for Slider {
    fn is_disabled(&self) -> bool {
        self.is_disabled
    }

    fn disable(&mut self) {
        self.is_disabled = true;
        self.is_dragging = false;
        self.mark_dirty();
    }

    fn enable(&mut self) {
        self.is_disabled = false;
        self.mark_dirty();
    }
}

impl Hideable for Slider {
    fn hide(&mut self) {
        self.is_hidden = true;
        self.is_dragging = false;
        self.mark_dirty();
    }

    fn show(&mut self) {
        self.is_hidden = false;
        self.mark_dirty();
    }

    fn is_hidden(&self) -> bool {
        self.is_hidden
    }
}

impl Focusable for Slider {
    fn can_unfocus(&self) -> bool {
        !self.is_dragging
    }

    fn focus(&mut self) {
        self.mark_dirty();
    }

    fn unfocus(&mut self) {
        self.is_dragging = false;
        self.mark_dirty();
    }
}

impl Interactable for Slider {
    fn consume_keyboard_press(&mut self, keyboard_press: DecodedKey) -> Option<Box<dyn FnOnce() -> ()>> {
        if self.is_disabled {
            return None;
        }

        match keyboard_press {
            DecodedKey::Unicode('+') => {
                let step = i32::try_from(self.steps).unwrap_or(i32::MAX);
                let new_value = self.value.get().saturating_add(step);
                self.update_value(new_value)
            }
            DecodedKey::Unicode('-') => {
                let step = i32::try_from(self.steps).unwrap_or(i32::MAX);
                let new_value = self.value.get().saturating_sub(step);
                self.update_value(new_value)
            }
            _ => None,
        }
    }

    fn consume_mouse_event(&mut self, mouse_event: &MouseEvent) -> Option<Box<dyn FnOnce() -> ()>> {
        if self.is_disabled {
            return None;
        }

        // Scroll (+)
        if mouse_event.scroll == ScrollDirection::Up || mouse_event.scroll == ScrollDirection::Right {
            let step = i32::try_from(self.steps).unwrap_or(i32::MAX);
            let new_value = self.value.get().saturating_add(step);
            return self.update_value(new_value);
        }

        // Scroll (-)
        if mouse_event.scroll == ScrollDirection::Down || mouse_event.scroll == ScrollDirection::Left {
            let step = i32::try_from(self.steps).unwrap_or(i32::MAX);
            let new_value = self.value.get().saturating_sub(step);
            return self.update_value(new_value);
        }

        // Update dragging state
        if mouse_event.buttons.left.is_pressed() {
            self.is_dragging = true;
        } else if mouse_event.buttons.left.is_released() {
            self.is_dragging = false;
        }

        // Handle mouse dragging
        if self.is_dragging {
            let mouse_x = i64::from(mouse_event.position.x) - i64::from(self.abs_rect_data.top_left.x);
            let usable_width = self.abs_rect_data.width.saturating_sub(HANDLE_WIDTH).max(1);
            let normalized_position = (mouse_x as f64 / f64::from(usable_width)).clamp(0.0, 1.0);
            let normalized_value = normalized_position * (f64::from(self.max) - f64::from(self.min)) + f64::from(self.min);

            let new_value = round(normalized_value) as i32;
            let new_value = new_value.clamp(self.min, self.max);

            if self.value.get() != new_value {
                return self.update_value(new_value);
            }
        }

        None
    }
}
