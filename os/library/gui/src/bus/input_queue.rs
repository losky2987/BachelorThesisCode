use alloc::{collections::VecDeque, format, vec::Vec};
use concurrent::shm::{shm_attach, shm_detach, shm_open, shm_unlink};
use core::{
    ptr,
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
};
use drawer::{rect_data::RectData, vertex::Vertex};
use syscall::return_vals::Errno;
use terminal::{DecodedKey, KeyCode};

use crate::component::assistant::mouse_state::{ButtonState, MouseButtonState, MouseEvent, ScrollDirection};

const INPUT_QUEUE_MAGIC: u32 = 0x4433_4951; // "D3IQ"
const INPUT_QUEUE_VERSION: u32 = 2;
const INPUT_QUEUE_CAPACITY: usize = 256;
const MAX_PENDING_EVENTS: usize = 1024;
const INPUT_QUEUE_READY: u32 = 1;

static NEXT_INPUT_QUEUE_GENERATION: AtomicU64 = AtomicU64::new(1);

const EVENT_KEYBOARD: u32 = 1;
const EVENT_MOUSE: u32 = 2;
const EVENT_FOCUS_NEXT: u32 = 3;
const EVENT_FOCUS_PREV: u32 = 4;
const EVENT_UNFOCUS: u32 = 5;
const EVENT_WINDOW_RECT: u32 = 6;

#[repr(C)]
struct InputQueueHeader {
    magic: u32,
    version: u32,
    header_size: u32,
    event_size: u32,
    capacity: u32,
    ready: AtomicU32,
    window_id: u64,
    generation: u64,
    write_sequence: AtomicU32,
    read_sequence: AtomicU32,
    keyboard_captured: AtomicU32,
    dropped_events: AtomicU32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RawInputEvent {
    kind: u32,
    data: [u32; 8],
}

const INPUT_QUEUE_HEADER_SIZE: usize = core::mem::size_of::<InputQueueHeader>();
const INPUT_QUEUE_SIZE: usize = INPUT_QUEUE_HEADER_SIZE + INPUT_QUEUE_CAPACITY * core::mem::size_of::<RawInputEvent>();

#[derive(Clone, Copy)]
struct InputQueueMetadata {
    magic: u32,
    version: u32,
    header_size: u32,
    event_size: u32,
    capacity: u32,
    window_id: u64,
    generation: u64,
}

pub enum DirectInputEvent {
    Keyboard(DecodedKey),
    Mouse(MouseEvent),
    FocusNext,
    FocusPrev,
    Unfocus,
    WindowRect(RectData),
}

/// Window Manager side of the per-window shared input queue.
///
/// The shared ring is strictly SPSC. If the application temporarily falls
/// behind, events remain in this process-local pending queue and are retried by
/// the compositor loop. Redundant pointer/geometry state is coalesced, and the
/// local queue is bounded so a stalled application cannot exhaust WM memory.
pub struct InputQueueWriter {
    window_id: usize,
    shm_ptr: *mut u8,
    pending: VecDeque<RawInputEvent>,
    unlink_on_drop: bool,
}

impl InputQueueWriter {
    pub fn create(window_id: usize) -> Result<Self, Errno> {
        Self::create_generation(window_id, next_generation())
    }

    /// Start a fresh queue generation for a replacement application.
    ///
    /// The old reader may remain attached to the unlinked object until its
    /// process exits, but it can no longer race with the new queue's sequence
    /// counters or consume the new application's initial geometry.
    pub fn recreate(mut self) -> Result<Self, Errno> {
        let name = queue_name(self.window_id);
        if self.unlink_on_drop {
            let _ = shm_unlink(&name);
            self.unlink_on_drop = false;
        }

        Self::create_generation(self.window_id, next_generation())
    }

    fn create_generation(window_id: usize, generation: u64) -> Result<Self, Errno> {
        let name = queue_name(window_id);
        let shm_id = match shm_open(&name, INPUT_QUEUE_SIZE, true) {
            Ok(shm_id) => shm_id,
            Err(Errno::EEXIST) => {
                let _ = shm_unlink(&name);
                shm_open(&name, INPUT_QUEUE_SIZE, true)?
            }
            Err(error) => return Err(error),
        };
        let shm_ptr = match shm_attach(shm_id, false) {
            Ok(shm_ptr) => shm_ptr,
            Err(error) => {
                let _ = shm_unlink(&name);
                return Err(error);
            }
        };
        unsafe {
            ptr::write(
                shm_ptr.cast::<InputQueueHeader>(),
                InputQueueHeader {
                    magic: INPUT_QUEUE_MAGIC,
                    version: INPUT_QUEUE_VERSION,
                    header_size: INPUT_QUEUE_HEADER_SIZE as u32,
                    event_size: core::mem::size_of::<RawInputEvent>() as u32,
                    capacity: INPUT_QUEUE_CAPACITY as u32,
                    ready: AtomicU32::new(0),
                    window_id: window_id as u64,
                    generation,
                    write_sequence: AtomicU32::new(0),
                    read_sequence: AtomicU32::new(0),
                    keyboard_captured: AtomicU32::new(0),
                    dropped_events: AtomicU32::new(0),
                },
            );
        }
        unsafe {
            (*shm_ptr.cast::<InputQueueHeader>()).ready.store(INPUT_QUEUE_READY, Ordering::Release);
        }

        Ok(Self {
            window_id,
            shm_ptr,
            pending: VecDeque::new(),
            unlink_on_drop: true,
        })
    }

    pub fn disconnect(&mut self) {
        self.pending.clear();
        self.header().keyboard_captured.store(0, Ordering::Release);
    }

    pub fn keyboard_captured(&self) -> bool {
        self.header().keyboard_captured.load(Ordering::Acquire) != 0
    }

    pub fn generation(&self) -> u64 {
        self.header().generation
    }

    pub fn dropped_events(&self) -> u32 {
        self.header().dropped_events.load(Ordering::Relaxed)
    }

    pub fn push_keyboard(&mut self, key: DecodedKey) {
        let (key_type, value) = match key {
            DecodedKey::Unicode(character) => (0, character as u32),
            DecodedKey::RawKey(key_code) => (1, key_code as u8 as u32),
        };
        self.push(RawInputEvent {
            kind: EVENT_KEYBOARD,
            data: [key_type, value, 0, 0, 0, 0, 0, 0],
        });
    }

    pub fn push_mouse(&mut self, event: &MouseEvent) {
        self.push(RawInputEvent {
            kind: EVENT_MOUSE,
            data: [
                event.position.x,
                event.position.y,
                encode_button(event.buttons.left),
                encode_button(event.buttons.right),
                encode_button(event.buttons.middle),
                encode_button(event.buttons.button4),
                encode_button(event.buttons.button5),
                encode_scroll(event.scroll),
            ],
        });
    }

    pub fn push_focus_next(&mut self) {
        self.push(RawInputEvent::control(EVENT_FOCUS_NEXT));
    }

    pub fn push_focus_prev(&mut self) {
        self.push(RawInputEvent::control(EVENT_FOCUS_PREV));
    }

    pub fn push_unfocus(&mut self) {
        self.push(RawInputEvent::control(EVENT_UNFOCUS));
    }

    pub fn push_window_rect(&mut self, rect: RectData) {
        self.push(RawInputEvent {
            kind: EVENT_WINDOW_RECT,
            data: [rect.top_left.x, rect.top_left.y, rect.width, rect.height, 0, 0, 0, 0],
        });
    }

    pub fn flush(&mut self) {
        while let Some(event) = self.pending.front().copied() {
            if !self.try_push_shared(event) {
                break;
            }
            self.pending.pop_front();
        }
    }

    fn push(&mut self, event: RawInputEvent) {
        self.flush();
        if self.pending.is_empty() && self.try_push_shared(event) {
            return;
        }

        if let Some(last) = self.pending.back_mut() {
            if last.can_be_replaced_by(&event) {
                *last = event;
                return;
            }
        }

        if self.pending.len() >= MAX_PENDING_EVENTS {
            let replaceable = if event.is_coalescible_state() {
                self.pending
                    .iter()
                    .position(|pending| pending.same_coalescing_class(&event))
                    .or_else(|| self.pending.iter().position(RawInputEvent::is_coalescible_state))
            } else {
                self.pending.iter().position(RawInputEvent::is_coalescible_state)
            };
            if let Some(index) = replaceable {
                self.pending.remove(index);
                self.record_drop();
            } else {
                // A stalled or dead application must not make the Window
                // Manager allocate an unbounded critical-event backlog.
                self.record_drop();
                return;
            }
        }

        self.pending.push_back(event);
    }

    fn record_drop(&self) {
        self.header().dropped_events.fetch_add(1, Ordering::Relaxed);
    }

    fn try_push_shared(&self, event: RawInputEvent) -> bool {
        let header = self.header();
        let write = header.write_sequence.load(Ordering::Relaxed);
        let read = header.read_sequence.load(Ordering::Acquire);
        if write.wrapping_sub(read) as usize >= INPUT_QUEUE_CAPACITY {
            return false;
        }

        let slot = write as usize % INPUT_QUEUE_CAPACITY;
        unsafe {
            ptr::write_volatile(self.events_ptr().add(slot), event);
        }
        header.write_sequence.store(write.wrapping_add(1), Ordering::Release);
        true
    }

    fn header(&self) -> &InputQueueHeader {
        unsafe { &*self.shm_ptr.cast::<InputQueueHeader>() }
    }

    fn events_ptr(&self) -> *mut RawInputEvent {
        unsafe { self.shm_ptr.add(INPUT_QUEUE_HEADER_SIZE).cast::<RawInputEvent>() }
    }
}

impl Drop for InputQueueWriter {
    fn drop(&mut self) {
        let _ = shm_detach(self.shm_ptr);
        if self.unlink_on_drop {
            let _ = shm_unlink(&queue_name(self.window_id));
        }
    }
}

/// Application/libgui side of the shared input queue.
pub struct InputQueueReader {
    shm_ptr: *mut u8,
    generation: u64,
}

impl InputQueueReader {
    pub fn open(window_id: usize, expected_generation: u64) -> Result<Self, Errno> {
        let shm_id = shm_open(&queue_name(window_id), INPUT_QUEUE_SIZE, false)?;
        // The consumer owns read_sequence, so this mapping cannot be read-only.
        let shm_ptr = shm_attach(shm_id, false)?;
        let mut reader = Self { shm_ptr, generation: 0 };
        let header = reader.header();
        if header.ready.load(Ordering::Acquire) != INPUT_QUEUE_READY {
            return Err(Errno::EAGAIN);
        }

        let metadata = read_metadata(shm_ptr);
        if metadata.magic != INPUT_QUEUE_MAGIC
            || metadata.version != INPUT_QUEUE_VERSION
            || metadata.header_size as usize != INPUT_QUEUE_HEADER_SIZE
            || metadata.event_size as usize != core::mem::size_of::<RawInputEvent>()
            || metadata.capacity as usize != INPUT_QUEUE_CAPACITY
            || metadata.window_id != window_id as u64
            || metadata.generation != expected_generation
        {
            // `reader` owns the mapping and detaches it on this early return.
            return Err(Errno::EINVAL);
        }
        reader.generation = metadata.generation;
        Ok(reader)
    }

    pub fn drain(&self) -> Vec<DirectInputEvent> {
        let mut events = Vec::new();
        self.drain_into(&mut events);
        events
    }

    pub fn drain_into(&self, events: &mut Vec<DirectInputEvent>) {
        let header = self.header();
        let mut read = header.read_sequence.load(Ordering::Relaxed);
        let write = header.write_sequence.load(Ordering::Acquire);
        let available = write.wrapping_sub(read).min(INPUT_QUEUE_CAPACITY as u32) as usize;
        events.clear();
        events.reserve(available);

        for _ in 0..available {
            let slot = read as usize % INPUT_QUEUE_CAPACITY;
            let raw = unsafe { ptr::read_volatile(self.events_ptr().add(slot)) };
            if let Some(event) = raw.decode() {
                events.push(event);
            }
            read = read.wrapping_add(1);
        }

        header.read_sequence.store(read, Ordering::Release);
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn set_keyboard_captured(&self, captured: bool) {
        self.header().keyboard_captured.store(u32::from(captured), Ordering::Release);
    }

    fn header(&self) -> &InputQueueHeader {
        unsafe { &*self.shm_ptr.cast::<InputQueueHeader>() }
    }

    fn events_ptr(&self) -> *const RawInputEvent {
        unsafe { self.shm_ptr.add(INPUT_QUEUE_HEADER_SIZE).cast::<RawInputEvent>() }
    }
}

impl Drop for InputQueueReader {
    fn drop(&mut self) {
        let _ = shm_detach(self.shm_ptr);
    }
}

impl RawInputEvent {
    const fn control(kind: u32) -> Self {
        Self { kind, data: [0; 8] }
    }

    fn is_mouse(&self) -> bool {
        self.kind == EVENT_MOUSE
    }

    fn is_window_rect(&self) -> bool {
        self.kind == EVENT_WINDOW_RECT
    }

    fn is_coalescible_mouse(&self) -> bool {
        self.kind == EVENT_MOUSE && self.data[7] == 0 && self.data[2..7].iter().all(|state| matches!(*state, 0 | 2))
    }

    fn is_coalescible_state(&self) -> bool {
        self.is_coalescible_mouse() || self.is_window_rect()
    }

    fn same_coalescing_class(&self, next: &Self) -> bool {
        (self.is_coalescible_mouse() && next.is_coalescible_mouse()) || (self.is_window_rect() && next.is_window_rect())
    }

    fn can_be_replaced_by(&self, next: &Self) -> bool {
        (self.is_coalescible_mouse() && next.is_mouse()) || (self.is_window_rect() && next.is_window_rect())
    }

    fn decode(self) -> Option<DirectInputEvent> {
        match self.kind {
            EVENT_KEYBOARD => decode_key(self.data[0], self.data[1]).map(DirectInputEvent::Keyboard),
            EVENT_MOUSE => Some(DirectInputEvent::Mouse(MouseEvent {
                position: Vertex::new(self.data[0], self.data[1]),
                buttons: MouseButtonState {
                    left: decode_button(self.data[2]),
                    right: decode_button(self.data[3]),
                    middle: decode_button(self.data[4]),
                    button4: decode_button(self.data[5]),
                    button5: decode_button(self.data[6]),
                },
                scroll: decode_scroll(self.data[7]),
            })),
            EVENT_FOCUS_NEXT => Some(DirectInputEvent::FocusNext),
            EVENT_FOCUS_PREV => Some(DirectInputEvent::FocusPrev),
            EVENT_UNFOCUS => Some(DirectInputEvent::Unfocus),
            EVENT_WINDOW_RECT => Some(DirectInputEvent::WindowRect(RectData {
                top_left: Vertex::new(self.data[0], self.data[1]),
                width: self.data[2],
                height: self.data[3],
            })),
            _ => None,
        }
    }
}

fn queue_name(window_id: usize) -> alloc::string::String {
    format!("gui_{}_inputqueue_v{}", window_id, INPUT_QUEUE_VERSION)
}

fn next_generation() -> u64 {
    loop {
        let generation = NEXT_INPUT_QUEUE_GENERATION.fetch_add(1, Ordering::Relaxed);
        if generation != 0 {
            return generation;
        }
    }
}

fn read_metadata(shm_ptr: *mut u8) -> InputQueueMetadata {
    let source = shm_ptr.cast::<InputQueueHeader>();
    unsafe {
        InputQueueMetadata {
            magic: ptr::read_volatile(ptr::addr_of!((*source).magic)),
            version: ptr::read_volatile(ptr::addr_of!((*source).version)),
            header_size: ptr::read_volatile(ptr::addr_of!((*source).header_size)),
            event_size: ptr::read_volatile(ptr::addr_of!((*source).event_size)),
            capacity: ptr::read_volatile(ptr::addr_of!((*source).capacity)),
            window_id: ptr::read_volatile(ptr::addr_of!((*source).window_id)),
            generation: ptr::read_volatile(ptr::addr_of!((*source).generation)),
        }
    }
}

fn encode_button(state: ButtonState) -> u32 {
    match state {
        ButtonState::None => 0,
        ButtonState::Pressed => 1,
        ButtonState::Down => 2,
        ButtonState::Released => 3,
    }
}

fn decode_button(state: u32) -> ButtonState {
    match state {
        1 => ButtonState::Pressed,
        2 => ButtonState::Down,
        3 => ButtonState::Released,
        _ => ButtonState::None,
    }
}

fn encode_scroll(direction: ScrollDirection) -> u32 {
    match direction {
        ScrollDirection::None => 0,
        ScrollDirection::Up => 1,
        ScrollDirection::Down => 2,
        ScrollDirection::Left => 3,
        ScrollDirection::Right => 4,
    }
}

fn decode_scroll(direction: u32) -> ScrollDirection {
    match direction {
        1 => ScrollDirection::Up,
        2 => ScrollDirection::Down,
        3 => ScrollDirection::Left,
        4 => ScrollDirection::Right,
        _ => ScrollDirection::None,
    }
}

const KEY_CODES: &[KeyCode] = &[
    KeyCode::Escape,
    KeyCode::F1,
    KeyCode::F2,
    KeyCode::F3,
    KeyCode::F4,
    KeyCode::F5,
    KeyCode::F6,
    KeyCode::F7,
    KeyCode::F8,
    KeyCode::F9,
    KeyCode::F10,
    KeyCode::F11,
    KeyCode::F12,
    KeyCode::PrintScreen,
    KeyCode::SysRq,
    KeyCode::ScrollLock,
    KeyCode::PauseBreak,
    KeyCode::Oem8,
    KeyCode::Key1,
    KeyCode::Key2,
    KeyCode::Key3,
    KeyCode::Key4,
    KeyCode::Key5,
    KeyCode::Key6,
    KeyCode::Key7,
    KeyCode::Key8,
    KeyCode::Key9,
    KeyCode::Key0,
    KeyCode::OemMinus,
    KeyCode::OemPlus,
    KeyCode::Backspace,
    KeyCode::Insert,
    KeyCode::Home,
    KeyCode::PageUp,
    KeyCode::NumpadLock,
    KeyCode::NumpadDivide,
    KeyCode::NumpadMultiply,
    KeyCode::NumpadSubtract,
    KeyCode::Tab,
    KeyCode::Q,
    KeyCode::W,
    KeyCode::E,
    KeyCode::R,
    KeyCode::T,
    KeyCode::Y,
    KeyCode::U,
    KeyCode::I,
    KeyCode::O,
    KeyCode::P,
    KeyCode::Oem4,
    KeyCode::Oem6,
    KeyCode::Oem5,
    KeyCode::Oem7,
    KeyCode::Delete,
    KeyCode::End,
    KeyCode::PageDown,
    KeyCode::Numpad7,
    KeyCode::Numpad8,
    KeyCode::Numpad9,
    KeyCode::NumpadAdd,
    KeyCode::CapsLock,
    KeyCode::A,
    KeyCode::S,
    KeyCode::D,
    KeyCode::F,
    KeyCode::G,
    KeyCode::H,
    KeyCode::J,
    KeyCode::K,
    KeyCode::L,
    KeyCode::Oem1,
    KeyCode::Oem3,
    KeyCode::Return,
    KeyCode::Numpad4,
    KeyCode::Numpad5,
    KeyCode::Numpad6,
    KeyCode::LShift,
    KeyCode::Z,
    KeyCode::X,
    KeyCode::C,
    KeyCode::V,
    KeyCode::B,
    KeyCode::N,
    KeyCode::M,
    KeyCode::OemComma,
    KeyCode::OemPeriod,
    KeyCode::Oem2,
    KeyCode::RShift,
    KeyCode::ArrowUp,
    KeyCode::Numpad1,
    KeyCode::Numpad2,
    KeyCode::Numpad3,
    KeyCode::NumpadEnter,
    KeyCode::LControl,
    KeyCode::LWin,
    KeyCode::LAlt,
    KeyCode::Spacebar,
    KeyCode::RAltGr,
    KeyCode::RWin,
    KeyCode::Apps,
    KeyCode::RControl,
    KeyCode::ArrowLeft,
    KeyCode::ArrowDown,
    KeyCode::ArrowRight,
    KeyCode::Numpad0,
    KeyCode::NumpadPeriod,
    KeyCode::Oem9,
    KeyCode::Oem10,
    KeyCode::Oem11,
    KeyCode::Oem12,
    KeyCode::Oem13,
    KeyCode::PrevTrack,
    KeyCode::NextTrack,
    KeyCode::Mute,
    KeyCode::Calculator,
    KeyCode::Play,
    KeyCode::Stop,
    KeyCode::VolumeDown,
    KeyCode::VolumeUp,
    KeyCode::WWWHome,
    KeyCode::PowerOnTestOk,
    KeyCode::TooManyKeys,
    KeyCode::RControl2,
    KeyCode::RAlt2,
];

fn decode_key(key_type: u32, value: u32) -> Option<DecodedKey> {
    match key_type {
        0 => char::from_u32(value).map(DecodedKey::Unicode),
        1 => KEY_CODES
            .iter()
            .copied()
            .find(|key_code| *key_code as u8 as u32 == value)
            .map(DecodedKey::RawKey),
        _ => None,
    }
}
