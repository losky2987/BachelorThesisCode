#![no_std]

extern crate alloc;

use alloc::{boxed::Box, rc::Rc, string::String};
use drawer::{rect_data::RectData, vertex::Vertex};

use gui::{
    api::current_component_frame, component::{
        assistant::signal::Signal,
        widget::{button::Button, input_field::InputField, label::Label},
    }, runtime::{executable::GAppExecutable, gapp_bootloader}
};

#[unsafe(no_mangle)]
pub fn main() {
    gapp_bootloader::run(SubmitLabel::run);
}

pub struct SubmitLabel;

impl GAppExecutable for SubmitLabel {
    fn run() {
        let surface = current_component_frame().expect("ComponentFrame not registered for app thread");

        let starting_text = String::from("");
        let input = Signal::new(starting_text.clone());
        let input_text = Rc::clone(&input);
        let submit_input = Rc::clone(&input);
        let submitted_text = Signal::new(starting_text.clone());

        let input_rect = RectData {
            top_left: Vertex::new(50, 100),
            width: 200,
            height: 75,
        };
        let input_field = surface.add(
            None,
            InputField::new(
                surface.scale_rect(input_rect).unwrap(),
                2,
                12,
                starting_text.clone(),
                Some(Box::new(move |new_text| {
                    input.set(new_text);
                })),
                None,
            ),
        ).unwrap();

        surface.add(None, Label::new(
            surface.scale_vertex(Vertex::new(50, 200)).unwrap(),
            2,
            Signal::new(String::from("Submitted Text: ")),
            None,
        )).unwrap();

        surface.add(None, Label::new(
            surface.scale_vertex(Vertex::new(500, 200)).unwrap(),
            2,
            Rc::clone(&submitted_text),
            None,
        )).unwrap();

        surface.add(None, Label::new(
            surface.scale_vertex(Vertex::new(500, 300)).unwrap(),
            2,
            Rc::clone(&input_text),
            None,
        )).unwrap();

        let submit_rect = RectData {
            top_left: Vertex::new(50, 300),
            width: 160,
            height: 50,
        };
        surface.add(None, Button::new(
            surface.scale_rect(submit_rect).unwrap(),
            submit_rect,
            Some(Signal::new(String::from("Submit"))),
            3,
            Some(Box::new(move || {
                let value = submit_input.get();
                submitted_text.set(value);

                if let Some(clearable) = input_field.write().as_clearable_mut() {
                    clearable.clear();
                }
            })),
            None,
        )).unwrap();
    }
}
