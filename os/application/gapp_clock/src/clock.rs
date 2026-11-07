#![no_std]

extern crate alloc;

use alloc::{boxed::Box, rc::Rc, string::ToString};
use core::cell::Cell;
use drawer::vertex::Vertex;
use time::{date, systime};

use gui::{
    api::{current_component_frame, register_current_loop_iter},
    component::{assistant::signal::Signal, widget::label::Label},
    runtime::{executable::GAppExecutable, gapp_bootloader},
};

#[unsafe(no_mangle)]
pub fn main() {
    gapp_bootloader::run(Clock::run);
}

pub struct Clock;

impl GAppExecutable for Clock {
    fn run() {
        let surface = current_component_frame().expect("ComponentFrame not registered for app thread");
        let date_val = date().format("%Y-%m-%d %H:%M:%S").to_string();

        let clock = Signal::new(date_val);
        let second_clock = Rc::clone(&clock);
        let last_check_ms = Cell::new(systime().num_milliseconds());

        let clock_component = Label::new(surface.scale_vertex(Vertex::new(200, 200)).unwrap(), 4, Rc::clone(&clock), None);
        surface.add(None, clock_component).unwrap();

        register_current_loop_iter(Box::new(move || {
            let now_ms = systime().num_milliseconds();
            let previous_check_ms = last_check_ms.get();
            if now_ms >= previous_check_ms && now_ms.saturating_sub(previous_check_ms) < 100 {
                return false;
            }
            last_check_ms.set(now_ms);

            let old_date = clock.get();
            let new_date = date().format("%Y-%m-%d %H:%M:%S").to_string();

            if old_date != new_date {
                clock.set(new_date);
                return true;
            }

            false
        }))
        .unwrap();

        let second_clock_component = Label::new(surface.scale_vertex(Vertex::new(200, 300)).unwrap(), 4, Rc::clone(&second_clock), None);
        surface.add(None, second_clock_component).unwrap();
    }
}
