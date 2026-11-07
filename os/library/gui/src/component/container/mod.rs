/*

This file is a part of Library gui.

namespace: libgui::component::container

original name: mod.rs (gui::component::container::mod.rs)

author: Xiaochen Xing <xixin100@hhu.de>

created: 2026-5-27

last changed: 2026-6-24

*/

pub mod basic;
pub mod layout;
pub mod styling;

use drawer::{rect_data::RectData, vertex::Vertex};

use crate::component::{assistant::signal::ComponentRef, Component};

/// A Component that can hold multiple child components.
pub trait Container: Component + FocusManager {
    fn add_child(&mut self, child: ComponentRef);

    fn remove_child(&mut self, id: usize);

    /// Scales a relative rect to the container and returns the absolute rect.
    fn scale_to_container(
        &self,
        rel_rect: RectData,
        min_dim: (u32, u32),
        max_dim: (u32, u32),
        maintain_aspect_ratio: bool,
    ) -> RectData;

    /// Scales a relative vertex to the container and returns the absolute vertex.
    fn scale_vertex_to_container(&self, rel_pos: Vertex) -> Vertex;

    fn scale_font_to_container(&self, font_size: usize) -> (u32, u32);

    fn scale_radius_to_window(&self, radius: u32, min_radius: u32) -> u32;

    /// Moves and scales the container to the given absolute rectangle.
    /// This should only be done on the root container to prevent layout issues.
    fn move_to(&mut self, abs_rect: RectData);
}

/// Allows the component (usually Containers) to manage their focused component
/// and provides an interface to change the focus.
pub trait FocusManager {
    fn get_focused_child(&self) -> Option<ComponentRef>;

    /// Returns the next focusable component or `None` if there are no more.
    fn focus_next_child(&mut self) -> Option<ComponentRef>;

    /// Returns the previous focusable component or `None` if there are no more.
    fn focus_prev_child(&mut self) -> Option<ComponentRef>;

    /// Returns the focusable component at the specified position or `None` if there is
    /// no component at the specified position.
    fn focus_child_at(&mut self, pos: Vertex) -> Option<ComponentRef>;
}
