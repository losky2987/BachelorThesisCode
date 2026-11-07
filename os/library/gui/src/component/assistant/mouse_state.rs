/*

This file is a part of Library gui.

namespace: libgui::component::assistant

original name: mouse_state.rs (gui::component::assistant::mouse_state.rs)

author: Xiaochen Xing <xixin100@hhu.de>

created: 2026-5-27

last changed: 2026-6-24

*/

use drawer::vertex::Vertex;

// Events that will be sent to components
pub struct MouseEvent {
    pub buttons: MouseButtonState,
    pub position: Vertex,
    pub scroll: ScrollDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    None,
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy)]
pub struct MouseButtonState {
    pub left: ButtonState,
    pub right: ButtonState,
    pub middle: ButtonState,
    pub button4: ButtonState,
    pub button5: ButtonState,
}

impl MouseButtonState {
    pub fn new() -> Self {
        Self {
            left: ButtonState::None,
            right: ButtonState::None,
            middle: ButtonState::None,
            button4: ButtonState::None,
            button5: ButtonState::None,
        }
    }
}

// None -> Pressed -> Down -> Released -> None
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ButtonState {
    None,
    Pressed,
    Down,
    Released,
}

impl ButtonState {
    pub fn next_state(&self, is_down: bool) -> ButtonState {
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
