#![no_std]

extern crate alloc;

use alloc::{boxed::Box, format, string::String};
use drawer::{rect_data::RectData, vertex::Vertex};
use graphic::color::Color;
use log::debug;

use gui::{
    api::{ComponentFrameHandle, component_ref, current_component_frame}, component::{
        assistant::signal::{ComponentRef, Signal},
        container::{
            basic::BasicContainer,
            layout::{AlignmentMode, ContainerLayoutBuilder, HorizontalDirection, StretchMode, VerticalDirection},
            styling::ContainerStylingBuilder,
        },
        styling::ComponentStylingBuilder,
        widget::button::Button,
    }, runtime::{executable::GAppExecutable, gapp_bootloader}
};

#[unsafe(no_mangle)]
pub fn main() {
    gapp_bootloader::run(LayoutApp::run);
}

pub struct LayoutApp;

impl GAppExecutable for LayoutApp {
    fn run() {
        let surface = current_component_frame().expect("ComponentFrame not registered for app thread");

        fn create_container(
            surface: &ComponentFrameHandle,
            parent: Option<ComponentRef>,
            log_rect_data: RectData,
            layout: Option<gui::component::container::layout::ContainerLayout>,
            styling: Option<gui::component::container::styling::ContainerStyling>,
        ) -> ComponentRef {
            surface.add(
                parent,
                component_ref(BasicContainer::new(
                    surface.scale_rect(log_rect_data).unwrap(),
                    layout,
                    styling,
                )),
            ).expect("failed to create container")
        }

        fn create_button(
            surface: &ComponentFrameHandle,
            parent: Option<ComponentRef>,
            x: u32,
            y: u32,
            width: u32,
            height: u32,
            label: &str,
        ) -> Option<ComponentRef> {
            let log_rect_data = RectData {
                top_left: Vertex { x, y },
                width,
                height,
            };

            surface.add(
                parent,
                Button::new(
                    surface.scale_rect(log_rect_data).unwrap(),
                    log_rect_data,
                    Some(Signal::new(String::from(label))),
                    1,
                    Some(Box::new(move || {
                        debug!("click!");
                    })),
                    Some(
                        ComponentStylingBuilder::new()
                            .maintain_aspect_ratio(true)
                            .build(),
                    ),
                ),
            ).ok()
        }

        let container_1 = create_container(
            &surface,
            None,
            RectData { top_left: Vertex { x: 50, y: 50 }, width: 300, height: 500 },
            Some(
                ContainerLayoutBuilder::new()
                    .alignment(AlignmentMode::Vertical(VerticalDirection::Top))
                    .stretch(StretchMode::Fill)
                    .build(),
            ),
            None,
        );

        let container_2 = create_container(
            &surface,
            Some(container_1.clone()),
            RectData { top_left: Vertex::zero(), width: 0, height: 100 },
            Some(
                ContainerLayoutBuilder::new()
                    .alignment(AlignmentMode::Horizontal(HorizontalDirection::Right))
                    .stretch(StretchMode::Fill)
                    .build(),
            ),
            Some(
                ContainerStylingBuilder::new()
                    .border_color(Color::new(255, 0, 0, 255))
                    .build(),
            ),
        );

        let container_3 = create_container(
            &surface,
            Some(container_1.clone()),
            RectData { top_left: Vertex::zero(), width: 0, height: 400 },
            Some(
                ContainerLayoutBuilder::new()
                    .alignment(AlignmentMode::Vertical(VerticalDirection::Bottom))
                    .stretch(StretchMode::Fill)
                    .build(),
            ),
            Some(
                ContainerStylingBuilder::new()
                    .border_color(Color::new(0, 255, 0, 255))
                    .maintain_aspect_ratio(true)
                    .build(),
            ),
        );

        let container_4 = create_container(
            &surface,
            Some(container_1.clone()),
            RectData { top_left: Vertex::zero(), width: 0, height: 100 },
            Some(
                ContainerLayoutBuilder::new()
                    .stretch(StretchMode::Fill)
                    .build(),
            ),
            Some(
                ContainerStylingBuilder::new()
                    .border_color(Color::new(255, 0, 255, 255))
                    .build(),
            ),
        );

        let grid_container = create_container(
            &surface,
            None,
            RectData { top_left: Vertex { x: 400, y: 50 }, width: 250, height: 250 },
            Some(
                ContainerLayoutBuilder::new()
                    .alignment(AlignmentMode::Grid(3))
                    .build(),
            ),
            Some(
                ContainerStylingBuilder::new()
                    .maintain_aspect_ratio(true)
                    .build(),
            ),
        );

        for i in 0..3 {
            let _ = create_button(&surface, Some(container_2.clone()), 0, 0, 150, 0, &format!("{}", i));
        }

        for i in 0..3 {
            let _ = create_button(&surface, Some(container_3.clone()), 0, 0, 0, 200, &format!("{}", i));
        }

        create_button(&surface, Some(container_4.clone()), 0, 0, 0, 0, "Hello!");

        for i in 0..9 {
            let _ = create_button(&surface, Some(grid_container.clone()), 0, 0, 200, 200, &format!("{}", i));
        }
    }
}
