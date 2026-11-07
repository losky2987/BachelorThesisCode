/*
    The workspace selection window is displayed above the windows
    and allows the user to switch workspaces or create/close them.
*/

use alloc::{
    boxed::Box,
    string::{String, ToString},
};
use drawer::{drawer::Drawer, rect_data::RectData, vertex::Vertex};
use graphic::{
    bitmap::Bitmap,
    color::{BLUE, GREY, INVISIBLE, RED},
};
use hashbrown::HashMap;

use gui::{
    api::screen_resolution,
    api::{send_window_manager_message, WindowManagerMessage},
};

use gui::{
    component::{
        assistant::{
            mouse_state::MouseEvent,
            signal::{ComponentRef, ComponentRefExt, Signal, Stateful},
        },
        container::{
            basic::BasicContainer,
            layout::{AlignmentMode, ContainerLayoutBuilder, HorizontalDirection, StretchMode},
            styling::ContainerStylingBuilder,
            Container,
        },
        styling::ComponentStylingBuilder,
        widget::button::Button,
        Component,
    },
    drawer::bitmap_drawer::BitmapDrawer,
};

pub struct WorkspaceSelectionWindow {
    abs_rect: RectData,
    root_container: Box<BasicContainer>,
    button_container: ComponentRef,
    action_container: ComponentRef,

    new_workspace_button: ComponentRef,
    close_workspace_button: ComponentRef,

    workspace_buttons: HashMap<usize, ComponentRef>,
    workspace_labels: HashMap<usize, Stateful<String>>,
    is_dirty: bool,
    last_active_workspace_id: Option<usize>,
}

impl WorkspaceSelectionWindow {
    pub fn new(abs_rect: RectData) -> Self {
        let screen_size = screen_resolution();
        let screen_rect = RectData {
            top_left: Vertex::zero(),
            width: screen_size.0,
            height: screen_size.1,
        };

        // Root container that will hold all buttons
        let mut root_container = Box::new(BasicContainer::new(
            screen_rect,
            Some(ContainerLayoutBuilder::new().stretch(StretchMode::Fill).build()),
            None,
        ));

        // Initial scaling to the window bounds
        root_container.move_to(abs_rect);

        // Container for the workspace buttons
        let button_container = Box::new(BasicContainer::new(
            RectData {
                top_left: Vertex::zero(),
                width: 0,
                height: 0,
            },
            Some(
                ContainerLayoutBuilder::new()
                    .alignment(AlignmentMode::Horizontal(HorizontalDirection::Left))
                    .stretch(StretchMode::Fill)
                    .build(),
            ),
            Some(ContainerStylingBuilder::new().show_border(false).build()),
        ));

        // Container for workspace actions (new, close)
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
            Some(ContainerStylingBuilder::new().show_border(false).build()),
        ));

        // Action buttons
        let rel_rect = RectData {
            top_left: Vertex::zero(),
            width: 50,
            height: 0,
        };

        let new_workspace_button = Button::new(
            rel_rect,
            rel_rect,
            Some(Signal::new(String::from("+"))),
            1,
            Some(Box::new(move || {
                let _ = send_window_manager_message(WindowManagerMessage::CreateNewWorkspace);
            })),
            Some(ComponentStylingBuilder::new().border_color(BLUE).background_color(BLUE.dim()).build()),
        );

        let close_workspace_button = Button::new(
            rel_rect,
            rel_rect,
            Some(Signal::new(String::from("X"))),
            1,
            Some(Box::new(move || {
                let _ = send_window_manager_message(WindowManagerMessage::CloseCurrentWorkspace);
            })),
            Some(ComponentStylingBuilder::new().border_color(RED).background_color(RED.dim()).build()),
        );

        action_container.add_child(close_workspace_button.clone());
        action_container.add_child(new_workspace_button.clone());

        let button_container = ComponentRef::from_component(button_container);
        let action_container = ComponentRef::from_component(action_container);

        root_container.add_child(button_container.clone());
        root_container.add_child(action_container.clone());

        Self {
            abs_rect,
            root_container,
            button_container,
            action_container,

            new_workspace_button,
            close_workspace_button,

            workspace_buttons: HashMap::new(),
            workspace_labels: HashMap::new(),
            is_dirty: true,
            last_active_workspace_id: None,
        }
    }

    pub fn draw(&mut self, active_workspace_id: Option<usize>) {
        let focus_changed = self.last_active_workspace_id != active_workspace_id;
        if !self.is_dirty && !self.root_container.is_dirty() && !focus_changed {
            return;
        }

        let focus_id = active_workspace_id
            .and_then(|workspace_id| self.workspace_buttons.get(&workspace_id))
            .map(|button| button.read().get_id());

        let mut bitmap = Bitmap {
            width: self.abs_rect.width,
            height: self.abs_rect.height,
            data: alloc::vec![
                INVISIBLE;
                (self.abs_rect.width * self.abs_rect.height) as usize
            ],
        };

        let mut bitmap_drawer = BitmapDrawer::new_with_origin(&mut bitmap, self.abs_rect.top_left);

        // This is rendered into a fresh offscreen bitmap, so make the redraw
        // complete whenever the workspace strip is dirty.
        self.root_container.mark_dirty();
        self.root_container.draw(focus_id, &mut bitmap_drawer);

        Drawer::draw_bitmap(self.abs_rect.top_left, &bitmap);
        self.last_active_workspace_id = active_workspace_id;
        self.is_dirty = false;
    }

    /// Adds a new button for the given stable workspace id.
    pub fn register_workspace(&mut self, workspace_id: usize) {
        let rel_rect = RectData {
            top_left: Vertex::zero(),
            width: 50,
            height: 0,
        };
        let label = Signal::new((self.workspace_buttons.len() + 1).to_string());

        let button = Button::new(
            rel_rect,
            rel_rect,
            Some(label.clone()),
            1,
            Some(Box::new(move || {
                let _ = send_window_manager_message(WindowManagerMessage::SwitchToWorkspace(workspace_id));
            })),
            Some(
                ComponentStylingBuilder::new()
                    .background_color(GREY.dim())
                    .focused_background_color(GREY)
                    .build(),
            ),
        );

        self.workspace_buttons.insert(workspace_id, button.clone());
        self.workspace_labels.insert(workspace_id, label);
        self.button_container.write().as_container_mut().unwrap().add_child(button);
        self.mark_dirty();
    }

    /// Removes the button for the given stable workspace id.
    pub fn unregister_workspace(&mut self, workspace_id: usize) {
        // Remove the button from the list
        let button = self.workspace_buttons.remove(&workspace_id);
        self.workspace_labels.remove(&workspace_id);

        if let Some(button) = button {
            self.button_container.write().as_container_mut().unwrap().remove_child(button.read().get_id());
            self.mark_dirty();
        }
    }

    /// Updates the visible, one-based workspace numbers while preserving the
    /// stable ids captured by the buttons' switch callbacks.
    pub fn renumber_workspaces(&mut self, workspace_ids: &[usize]) {
        for (index, workspace_id) in workspace_ids.iter().enumerate() {
            if let Some(label) = self.workspace_labels.get(workspace_id) {
                label.set((index + 1).to_string());
            }
        }
        self.mark_dirty();
    }

    pub fn handle_mouse_event(&mut self, mouse_event: &MouseEvent) -> Option<Box<dyn FnOnce()>> {
        // Action buttons
        {
            let new_component = self.action_container.write().as_container_mut().unwrap().focus_child_at(mouse_event.position);

            if let Some(new_component) = new_component {
                if let Some(interactable) = new_component.write().as_interactable_mut() {
                    return interactable.consume_mouse_event(mouse_event);
                }
            }
        }

        // Workspace buttons
        {
            let new_component = self.button_container.write().as_container_mut().unwrap().focus_child_at(mouse_event.position);

            if let Some(new_component) = new_component {
                if let Some(interactable) = new_component.write().as_interactable_mut() {
                    return interactable.consume_mouse_event(mouse_event);
                }
            }
        }

        return None;
    }

    pub fn mark_dirty(&mut self) {
        self.is_dirty = true;
        self.root_container.mark_dirty();
    }
}
