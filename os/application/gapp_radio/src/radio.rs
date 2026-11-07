#![no_std]

extern crate alloc;

use alloc::{boxed::Box, rc::Rc, string::{String, ToString}};
use drawer::vertex::Vertex;

use gui::{
    api::{component_ref, current_component_frame}, component::{
        assistant::signal::Signal,
        styling::ComponentStylingBuilder,
        widget::{label::Label, radio_button::RadioButtonGroup},
    }, runtime::{executable::GAppExecutable, gapp_bootloader}
};

#[unsafe(no_mangle)]
pub fn main() {
    gapp_bootloader::run(RadioButtonApp::run);
}

pub struct RadioButtonApp;

impl GAppExecutable for RadioButtonApp {
    fn run() {
        let surface = current_component_frame().expect("ComponentFrame not registered for app thread");
        let option = Signal::new(String::from("1"));
        let option_radio_buttons = Rc::clone(&option);

        surface.add(None, component_ref(RadioButtonGroup::new(
            3,
            surface.scale_vertex(Vertex::new(100, 50)).unwrap(),
            surface.scale_radius(20),
            5,
            Signal::new(1),
            Some(Rc::new(Box::new(move |selected_option: usize| {
                option_radio_buttons.set(selected_option.to_string());
            }))),
            Some(
                ComponentStylingBuilder::new()
                    .maintain_aspect_ratio(true)
                    .build(),
            ),
        ))).expect("Failed to create radio button group");

        surface.add(None, Label::new(
            surface.scale_vertex(Vertex::new(80, 100)).unwrap(),
            4,
            Signal::new(String::from("Selected option: ")),
            None,
        )).expect("Failed to create label");

        surface.add(None, Label::new(
            surface.scale_vertex(Vertex::new(450, 100)).unwrap(),
            4,
            Rc::clone(&option),
            None,
        )).expect("Failed to create label");
    }
}
