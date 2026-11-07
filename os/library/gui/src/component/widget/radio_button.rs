use alloc::{boxed::Box, rc::Rc};
use drawer::{rect_data::RectData, vertex::Vertex};
use terminal::DecodedKey;

use crate::{
    component::{
        Casts, Component, Disableable, Focusable, Hideable, Interactable,
        assistant::{
            config::INTERACT_BUTTON,
            mouse_state::MouseEvent,
            signal::{ComponentRef, ComponentRefExt, Stateful},
        },
        container::{
            Container,
            basic::BasicContainer,
            layout::{AlignmentMode, ContainerLayoutBuilder, FitMode, HorizontalDirection},
            styling::ContainerStylingBuilder,
        },
        styling::ComponentStyling,
    },
    drawer::bitmap_drawer::BitmapDrawer,
    api::next_component_id,
};

pub struct RadioButton {
    id: usize,

    abs_center: Vertex,
    rel_center: Vertex,
    abs_radius: u32,
    rel_radius: u32,
    drawn_rect_data: RectData,

    button_index: usize,
    selected_button_index: Stateful<usize>,
    on_change: Option<Rc<Box<dyn Fn(usize) -> ()>>>,

    is_disabled: bool,
    is_hidden: bool,
    is_dirty: bool,

    styling: ComponentStyling,
}

impl RadioButton {
    pub fn new(
        rel_center: Vertex, rel_radius: u32, button_index: usize, selected_button_index: Stateful<usize>, on_change: Option<Rc<Box<dyn Fn(usize) -> ()>>>,
        styling: Option<ComponentStyling>,
    ) -> ComponentRef {
        let radio_button = Box::new(Self {
            id: next_component_id(),
            abs_center: Vertex::zero(),
            rel_center,
            abs_radius: 0,
            rel_radius,
            drawn_rect_data: RectData::zero(),

            button_index,
            selected_button_index: selected_button_index.clone(),
            on_change,

            is_disabled: false,
            is_hidden: false,
            is_dirty: true,
            styling: styling.unwrap_or_default(),
        });

        // Register the component in the signal
        let component = ComponentRef::from_component(radio_button);
        selected_button_index.register_component(component.clone());

        component
    }

    fn handle_click(&mut self) -> Option<Box<dyn FnOnce() -> ()>> {
        let button_index = self.button_index;
        let selected_button_index = self.selected_button_index.clone();
        let on_change = self.on_change.clone();

        return Some(Box::new(move || {
            if let Some(f) = on_change {
                f(button_index);
            }

            selected_button_index.set(button_index);
        }));
    }
}

impl Component for RadioButton {
    fn draw(&mut self, focus_id: Option<usize>, drawer: &mut BitmapDrawer) {
        if self.is_hidden {
            self.is_dirty = false;
            return;
        }

        let styling = &self.styling;
        let is_focused = focus_id == Some(self.id);

        let border_color = if self.is_disabled {
            styling.disabled_border_color
        } else if is_focused {
            styling.focused_border_color
        } else {
            styling.border_color
        };

        drawer.draw_circle(self.abs_center, self.abs_radius, border_color);

        self.drawn_rect_data = self.get_abs_rect_data();

        // Is the button selected?
        if self.selected_button_index.get() == self.button_index {
            let inner_rad = (self.abs_radius as f32 * 0.5) as u32;
            drawer.draw_filled_circle(self.abs_center, inner_rad, border_color, None);
        }

        self.is_dirty = false;
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
        RectData {
            top_left: self.abs_center.sub(self.abs_radius, self.abs_radius),
            width: self.abs_radius.saturating_mul(2),
            height: self.abs_radius.saturating_mul(2),
        }
    }

    fn get_drawn_rect_data(&self) -> RectData {
        self.drawn_rect_data
    }

    fn rescale_to_container(&mut self, parent: &dyn Container) {
        self.abs_center = parent.scale_vertex_to_container(self.rel_center);
        self.abs_radius = parent.scale_radius_to_window(self.rel_radius, 7);

        // HACK: This should be done by the container
        self.abs_center = self.abs_center.add(self.abs_radius, self.abs_radius);
    }
}

impl Focusable for RadioButton {
    fn focus(&mut self) {
        self.mark_dirty();
    }

    fn unfocus(&mut self) {
        self.mark_dirty();
    }
}

impl Interactable for RadioButton {
    fn consume_keyboard_press(&mut self, keyboard_press: DecodedKey) -> Option<Box<dyn FnOnce() -> ()>> {
        if keyboard_press == INTERACT_BUTTON && !self.is_disabled {
            return self.handle_click();
        }

        return None;
    }

    fn consume_mouse_event(&mut self, mouse_event: &MouseEvent) -> Option<Box<dyn FnOnce() -> ()>> {
        if mouse_event.buttons.left.is_pressed() && !self.is_disabled {
            return self.handle_click();
        }

        return None;
    }
}

impl Casts for RadioButton {
    fn as_disableable(&self) -> Option<&dyn Disableable> {
        Some(self)
    }

    fn as_disableable_mut(&mut self) -> Option<&mut dyn Disableable> {
        Some(self)
    }

    fn as_hideable(&self) -> Option<&dyn Hideable> {
        Some(self)
    }

    fn as_hideable_mut(&mut self) -> Option<&mut dyn Hideable> {
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

    fn as_interactable_mut(&mut self) -> Option<&mut dyn Interactable> {
        Some(self)
    }
}

impl Disableable for RadioButton {
    fn is_disabled(&self) -> bool {
        self.is_disabled
    }

    fn disable(&mut self) {
        self.is_disabled = true;
        self.mark_dirty();
    }

    fn enable(&mut self) {
        self.is_disabled = false;
        self.mark_dirty();
    }
}

impl Hideable for RadioButton {
    fn is_hidden(&self) -> bool {
        self.is_hidden
    }

    fn hide(&mut self) {
        self.is_hidden = true;
        self.mark_dirty();
    }

    fn show(&mut self) {
        self.is_hidden = false;
        self.mark_dirty();
    }
}

pub struct RadioButtonGroup;

impl RadioButtonGroup {
    pub fn new(
        num_buttons: usize, rel_center: Vertex, rel_radius: u32, spacing: u32, selected_button_index: Stateful<usize>,
        on_change: Option<Rc<Box<dyn Fn(usize) -> ()>>>, styling: Option<ComponentStyling>,
    ) -> BasicContainer {
        let mut button_container = BasicContainer::new(
            RectData {
                top_left: rel_center,
                width: 0,
                height: 0,
            },
            Some(
                ContainerLayoutBuilder::new()
                    .alignment(AlignmentMode::Horizontal(HorizontalDirection::Left))
                    .fit(FitMode::GrowAndShrink)
                    .build(),
            ),
            Some(ContainerStylingBuilder::new().child_padding(spacing).show_border(false).build()),
        );

        // Create and add radio buttons
        for i in 0..num_buttons {
            let radio_button = RadioButton::new(Vertex::zero(), rel_radius, i, selected_button_index.clone(), on_change.clone(), styling.clone());

            button_container.add_child(radio_button);
        }

        button_container
    }
}
