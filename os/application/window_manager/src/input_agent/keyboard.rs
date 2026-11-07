use input::keyboard;
use pc_keyboard::{
    layouts::{AnyLayout, De105Key},
    EventDecoder, HandleControl, KeyCode, KeyState,
};
use spin::mutex::Mutex;
use terminal::DecodedKey;

pub struct KeyboardInput {
    pub key: DecodedKey,
    pub control_down: bool,
}

struct KeyboardState {
    decoder: EventDecoder<AnyLayout>,
    control_down: bool,
}

/// Author: Sebastian Keller
///
/// Keyboard decoder that reads directly from the keyboard.
/// Reading from the terminal caused a heavy performance hit, due to the current lack of ipc tools.
pub struct KeyboardDecoder {
    state: Mutex<KeyboardState>,
}

impl KeyboardDecoder {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(KeyboardState {
                decoder: EventDecoder::new(AnyLayout::De105Key(De105Key), HandleControl::Ignore),
                control_down: false,
            }),
        }
    }

    /// Author: Sebastian Keller
    ///
    /// Returns: Decoded key of user input (Unicode or Keycode)
    ///
    /// Returns: None if there is no user input
    pub fn read_decoded(&self) -> Option<KeyboardInput> {
        let key_event = keyboard::read_raw(false)?;
        let mut state = self.state.lock();

        if matches!(key_event.code, KeyCode::LControl | KeyCode::RControl) {
            state.control_down = key_event.state == KeyState::Down;
        }

        let control_down = state.control_down;
        let key = state.decoder.process_keyevent(key_event)?;

        Some(KeyboardInput { key, control_down })
    }
}
