#![no_std]

extern crate alloc;

use alloc::{
    boxed::Box,
    rc::Rc,
    string::{String, ToString},
};
use drawer::{rect_data::RectData, vertex::Vertex};
use spin::RwLock;

use gui::{
    api::current_component_frame,
    component::{
        assistant::signal::{ComponentRef, Signal},
        styling::ComponentStylingBuilder,
        widget::{button::Button, checkbox::Checkbox},
    },
    runtime::{executable::GAppExecutable, gapp_bootloader},
};

#[unsafe(no_mangle)]
pub fn main() {
    gapp_bootloader::run(Counter::run);
}

pub struct Counter;

impl GAppExecutable for Counter {
    fn run() {
        let surface = current_component_frame().expect("ComponentFrame not registered for app thread");

        let label = Signal::new(String::from("0"));
        let label_inc = Rc::clone(&label);
        let label_reset = Rc::clone(&label);

        let counter_button: Rc<RwLock<Option<ComponentRef>>> = Rc::new(RwLock::new(None));
        let reset_button: Rc<RwLock<Option<ComponentRef>>> = Rc::new(RwLock::new(None));

        let counter_button_checkbox = Rc::clone(&counter_button);
        let reset_button_checkbox = Rc::clone(&reset_button);

        let checkbox_rect = RectData {
            top_left: Vertex::new(200, 50),
            width: 25,
            height: 25,
        };
        let checkbox = Checkbox::new(
            surface.scale_rect(checkbox_rect).unwrap(),
            checkbox_rect,
            Signal::new(true),
            Some(Box::new(move |checked: bool| {
                if let Some(counter_button) = counter_button_checkbox.read().as_ref() {
                    if let Some(disableable) = counter_button.write().as_disableable_mut() {
                        if checked {
                            disableable.enable();
                        } else {
                            disableable.disable();
                        }
                    }
                }

                if let Some(reset_button) = reset_button_checkbox.read().as_ref() {
                    if let Some(hideable) = reset_button.write().as_hideable_mut() {
                        if checked {
                            hideable.show();
                        } else {
                            hideable.hide();
                        }
                    }
                }
            })),
            Some(ComponentStylingBuilder::new().maintain_aspect_ratio(true).build()),
        );
        surface.add(None, checkbox).unwrap();

        let counter_rect = RectData {
            top_left: Vertex::new(200, 150),
            width: 200,
            height: 50,
        };
        *counter_button.write() = Some(
            surface
                .add(
                    None,
                    Button::new(
                        surface.scale_rect(counter_rect).unwrap(),
                        counter_rect,
                        Some(Rc::clone(&label)),
                        1,
                        Some(Box::new(move || {
                            let old = label_inc.get().parse::<usize>().unwrap_or(0);
                            label_inc.set(old.saturating_add(1).to_string());
                        })),
                        None,
                    ),
                )
                .unwrap(),
        );

        let reset_rect = RectData {
            top_left: Vertex::new(200, 250),
            width: 200,
            height: 50,
        };
        *reset_button.write() = Some(
            surface
                .add(
                    None,
                    Button::new(
                        surface.scale_rect(reset_rect).unwrap(),
                        reset_rect,
                        Some(Signal::new(String::from("Reset"))),
                        1,
                        Some(Box::new(move || {
                            label_reset.set(String::from("0"));
                        })),
                        None,
                    ),
                )
                .unwrap(),
        );
    }
}
