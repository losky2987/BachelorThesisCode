/*
    Processes mouse packets into mouse events and
    manages the mouse state.
*/

use alloc::vec;
use drawer::drawer::Drawer;
use input::mouse::MousePacket;

use crate::config::DEFAULT_FG_COLOR;

pub use drawer::vertex::Vertex;

use gui::component::assistant::mouse_state::{MouseButtonState, MouseEvent, ScrollDirection};
use gui::api::screen_resolution;

// None -> Pressed -> Down -> Released -> None
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ButtonState {
    None,
    Pressed,
    Down,
    Released,
}

impl ButtonState {
    fn next_state(&self, is_down: bool) -> ButtonState {
        match (*self, is_down) {
            // Button is pressed (None/Released -> Pressed -> Down/Released)
            (ButtonState::None, true) => ButtonState::Pressed,
            (ButtonState::Pressed, true) => ButtonState::Down,
            (ButtonState::Released, true) => ButtonState::Pressed,

            // Button is released (Down/Pressed -> Released -> None/Presed)
            (ButtonState::Pressed, false) => ButtonState::Released,
            (ButtonState::Down, false) => ButtonState::Released,
            (ButtonState::Released, false) => ButtonState::None,

            // Maintain current state in other cases
            (state, _) => state,
        }
    }

    pub fn is_up(&self) -> bool {
        matches!(self, ButtonState::None)
    }

    pub fn is_pressed(&self) -> bool {
        matches!(self, ButtonState::Pressed)
    }

    pub fn is_down(&self) -> bool {
        matches!(self, ButtonState::Down)
    }

    pub fn is_released(&self) -> bool {
        matches!(self, ButtonState::Released)
    }
}

pub struct MouseState {
    position: Vertex,
    last_position: Vertex,
    cursor_dirty: bool,

    buttons: MouseButtonState,
}

impl MouseState {
    pub fn new() -> Self {
        Self {
            position: Vertex::new(0, 0),
            last_position: Vertex::new(0, 0),
            cursor_dirty: true,

            buttons: MouseButtonState::new(),
        }
    }

    pub fn process(&mut self, mouse_packet: &MousePacket) -> MouseEvent {
        let old_position = self.position;
        self.position = self.position.add_signed(mouse_packet.dx as i32, -mouse_packet.dy as i32);
        let screen = screen_resolution();
        self.position.x = self.position.x.min(screen.0.saturating_sub(1));
        self.position.y = self.position.y.min(screen.1.saturating_sub(1));
        if self.position.x != old_position.x || self.position.y != old_position.y {
            self.cursor_dirty = true;
        }

        // Update button states
        self.buttons = MouseButtonState {
            left: self.buttons.left.next_state(mouse_packet.left_button_down()),
            right: self.buttons.right.next_state(mouse_packet.right_button_down()),
            middle: self.buttons.middle.next_state(mouse_packet.middle_button_down()),
            button4: self.buttons.button4.next_state(mouse_packet.button4_down()),
            button5: self.buttons.button5.next_state(mouse_packet.button5_down()),
        };

        // Horizontal scrolling sends -2 or 2 for left/right
        let scroll_direction = match mouse_packet.dz {
            -1 => ScrollDirection::Up,
            1 => ScrollDirection::Down,
            -2 => ScrollDirection::Right,
            2 => ScrollDirection::Left,
            _ => ScrollDirection::None,
        };

        // Print button states
        // debug!("Scroll: {:?}, Button 4: {}, Button 5: {}", scroll_direction, mouse_packet.button4_down(), mouse_packet.button5_down());

        MouseEvent {
            buttons: self.buttons,
            position: self.position,
            scroll: scroll_direction,
        }
    }

    pub fn position(&self) -> Vertex {
        self.position
    }

    pub fn mark_dirty(&mut self) {
        self.cursor_dirty = true;
    }

    pub fn draw_cursor(&mut self) {
        if !self.cursor_dirty {
            return;
        }

        Drawer::flush_lines(self.last_position.y, 11);

        Drawer::draw_polygon_direct(
            vec![
                Vertex::new(self.position.x, self.position.y),
                Vertex::new(self.position.x.saturating_add(10), self.position.y.saturating_add(4)),
                Vertex::new(self.position.x.saturating_add(4), self.position.y.saturating_add(10)),
            ],
            DEFAULT_FG_COLOR,
        );

        self.last_position = self.position;
        self.cursor_dirty = false;
    }
}
