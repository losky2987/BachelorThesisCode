#![no_std]

extern crate alloc;

use alloc::{boxed::Box, rc::Rc, string::ToString};
use drawer::{rect_data::RectData, vertex::Vertex};
use graphic::{bitmap::{Bitmap, ScalingMode}, color::Color};
use spin::RwLock;

use gui::{
    api::{component_ref, current_component_frame}, component::{
        assistant::signal::{ComponentRef, Signal, Stateful},
        styling::ComponentStylingBuilder,
        widget::{bitmap::BitmapGraphic, label::Label, slider::Slider},
    }, runtime::{executable::GAppExecutable, gapp_bootloader}
};

#[unsafe(no_mangle)]
pub fn main() {
    gapp_bootloader::run(SliderApp::run);
}

pub struct SliderApp;

impl GAppExecutable for SliderApp {
    fn run() {
        let surface = current_component_frame().expect("ComponentFrame not registered for app thread");

        let initial_value = 1;
        let value = Signal::new(initial_value);
        let last_value: Stateful<i32> = Signal::new(initial_value);

        let label = Signal::new(initial_value.to_string());
        let label_slider = Rc::clone(&label);

        let bitmap: Rc<RwLock<Option<ComponentRef>>> = Rc::new(RwLock::new(None));

        let gradient_bitmap = Bitmap {
            width: 20,
            height: 10,
            data: (0..200)
                .map(|i| {
                    let intensity = ((i % 20) as f32 / 19.0 * 255.0) as u8;
                    Color { red: intensity, green: intensity, blue: 255, alpha: 255 }
                })
                .collect(),
        };

        surface.add(None, Label::new(
            surface.scale_vertex(Vertex::new(50, 100)).unwrap(),
            2,
            label,
            None,
        )).unwrap();

        let bitmap_slider = Rc::clone(&bitmap);
        let slider_rect = RectData {
            top_left: Vertex::new(50, 150),
            width: 200,
            height: 50,
        };
        surface.add(None, Slider::new(
            surface.scale_rect(slider_rect).unwrap(),
            slider_rect,
            Some(Box::new(move |value| {
                label_slider.set(value.to_string());

                if let Some(bitmap) = bitmap_slider.read().as_ref() {
                    if let Some(resizeable) = bitmap.write().as_resizable_mut() {
                        let factor = value as f64 / last_value.get() as f64;
                        resizeable.rescale(factor);
                        last_value.set(value);
                    }
                }
            })),
            value,
            1,
            3,
            1,
            None,
        )).unwrap();

        let graphic_rect = RectData {
            top_left: Vertex::new(50, 350),
            width: 50,
            height: 25,
        };
        *bitmap.write() = Some(surface.add(
            None,
            component_ref(BitmapGraphic::new(
                surface.scale_rect(graphic_rect).unwrap(),
                graphic_rect,
                gradient_bitmap,
                ScalingMode::Bilinear,
                Some(ComponentStylingBuilder::new()
                    .maintain_aspect_ratio(true)
                    .build()),
            )),
        ).unwrap());
    }
}
