#![no_std]

extern crate alloc;

use alloc::{
    boxed::Box,
    format,
    rc::Rc,
    string::{String, ToString},
};
use drawer::{rect_data::RectData, vertex::Vertex};
use graphic::color::{Color, GREY, RED, WHITE};
use spin::rwlock::RwLock;

use gui::{
    api::{ComponentFrameHandle, component_ref, current_component_frame},
    component::{
        assistant::signal::{ComponentRef, Signal},
        container::{
            basic::BasicContainer,
            layout::{AlignmentMode, ContainerLayoutBuilder, HorizontalDirection, StretchMode, VerticalDirection},
            styling::ContainerStylingBuilder,
        },
        styling::ComponentStylingBuilder,
        widget::{button::Button, label::Label},
    },
    runtime::{executable::GAppExecutable, gapp_bootloader},
};

#[unsafe(no_mangle)]
pub fn main() {
    gapp_bootloader::run(Calculator::run);
}

pub struct Calculator;
const MAX_DISPLAY_CHARS: usize = 32;

#[derive(Clone, Copy)]
enum CalculatorOperation {
    None,
    Add,
    Subtract,
    Multiply,
    Divide,
}

impl GAppExecutable for Calculator {
    fn run() {
        let surface = current_component_frame().expect("ComponentFrame not registered for app thread");

        let display_text = Signal::new(String::from("0"));
        let equals_button: Rc<RwLock<Option<ComponentRef>>> = Rc::new(RwLock::new(None));

        let current_operator = Rc::new(RwLock::<CalculatorOperation>::new(CalculatorOperation::None));
        let stored_value: Rc<RwLock<f64>> = Rc::new(RwLock::new(0.0));

        let display_sign = Rc::clone(&display_text);
        let display_decimal = Rc::clone(&display_text);

        let equals_clone = Rc::clone(&display_text);
        let equals_operator_clone = Rc::clone(&current_operator);
        let equals_stored_clone = Rc::clone(&stored_value);

        fn create_container(
            surface: &ComponentFrameHandle, parent: Option<ComponentRef>, log_rect_data: RectData,
            layout: Option<gui::component::container::layout::ContainerLayout>, styling: Option<gui::component::container::styling::ContainerStyling>,
        ) -> ComponentRef {
            surface
                .add(
                    parent,
                    component_ref(BasicContainer::new(surface.scale_rect(log_rect_data).unwrap(), layout, styling)),
                )
                .unwrap()
        }

        let content_container = create_container(
            &surface,
            None,
            RectData {
                top_left: Vertex { x: 50, y: 50 },
                width: 250,
                height: 400,
            },
            Some(
                ContainerLayoutBuilder::new()
                    .alignment(AlignmentMode::Vertical(VerticalDirection::Top))
                    .stretch(StretchMode::Fill)
                    .build(),
            ),
            Some(
                ContainerStylingBuilder::new()
                    .maintain_aspect_ratio(true)
                    .show_border(false)
                    .border_color(RED)
                    .build(),
            ),
        );

        surface
            .add(
                Some(content_container.clone()),
                Label::new(surface.scale_vertex(Vertex::zero()).unwrap(), 1, Rc::clone(&display_text), None),
            )
            .unwrap();

        let button_container = create_container(
            &surface,
            Some(content_container.clone()),
            RectData {
                top_left: Vertex { x: 0, y: 50 },
                width: 0,
                height: 500,
            },
            Some(ContainerLayoutBuilder::new().alignment(AlignmentMode::Grid(4)).build()),
            Some(ContainerStylingBuilder::new().maintain_aspect_ratio(true).show_border(false).build()),
        );

        let action_container = create_container(
            &surface,
            Some(content_container.clone()),
            RectData {
                top_left: Vertex::zero(),
                width: 0,
                height: 100,
            },
            Some(
                ContainerLayoutBuilder::new()
                    .alignment(AlignmentMode::Horizontal(HorizontalDirection::Left))
                    .stretch(StretchMode::Fill)
                    .build(),
            ),
            Some(ContainerStylingBuilder::new().maintain_aspect_ratio(true).show_border(false).build()),
        );

        let create_number_button = |parent: ComponentRef, number: usize| {
            let text_clone = Rc::clone(&display_text);
            let log_rect_data = RectData {
                top_left: Vertex::zero(),
                width: 200,
                height: 200,
            };

            surface
                .add(
                    Some(parent),
                    Button::new(
                        surface.scale_rect(log_rect_data).unwrap(),
                        log_rect_data,
                        Some(Signal::new(number.to_string())),
                        1,
                        Some(Box::new(move || {
                            let value = text_clone.get();
                            if value.len() >= MAX_DISPLAY_CHARS {
                                return;
                            } else if value == "0" {
                                text_clone.set(number.to_string());
                            } else {
                                text_clone.set(format!("{}{}", value, number));
                            }
                        })),
                        Some(
                            ComponentStylingBuilder::new()
                                .background_color(GREY.bright())
                                .focused_background_color(GREY.bright())
                                .text_color(WHITE.bright())
                                .focused_text_color(WHITE.bright())
                                .maintain_aspect_ratio(true)
                                .build(),
                        ),
                    ),
                )
                .unwrap();
        };

        let create_operator_button = |parent: ComponentRef, label: &str, operation: CalculatorOperation| {
            let text_clone = Rc::clone(&display_text);
            let stored_value_clone = Rc::clone(&stored_value);
            let operator_clone = Rc::clone(&current_operator);
            let equals_button_in_operator = Rc::clone(&equals_button);
            let log_rect_data = RectData {
                top_left: Vertex::zero(),
                width: 200,
                height: 200,
            };

            surface
                .add(
                    Some(parent),
                    Button::new(
                        surface.scale_rect(log_rect_data).unwrap(),
                        log_rect_data,
                        Some(Signal::new(label.to_string())),
                        1,
                        Some(Box::new(move || {
                            let value = text_clone.get();
                            let mut stored = stored_value_clone.write();
                            *stored = value.parse::<f64>().ok().filter(|value| value.is_finite()).unwrap_or(0.0);

                            text_clone.set(String::from("0"));

                            let mut operator = operator_clone.write();
                            *operator = operation;

                            if let Some(equals_button) = equals_button_in_operator.read().as_ref() {
                                if let Some(disableable) = equals_button.write().as_disableable_mut() {
                                    disableable.enable();
                                }
                            }
                        })),
                        Some(ComponentStylingBuilder::new().maintain_aspect_ratio(true).build()),
                    ),
                )
                .unwrap();
        };

        create_number_button(button_container.clone(), 0);
        create_number_button(button_container.clone(), 1);
        create_number_button(button_container.clone(), 2);
        create_operator_button(button_container.clone(), "+", CalculatorOperation::Add);

        create_number_button(button_container.clone(), 3);
        create_number_button(button_container.clone(), 4);
        create_number_button(button_container.clone(), 5);
        create_operator_button(button_container.clone(), "-", CalculatorOperation::Subtract);

        create_number_button(button_container.clone(), 6);
        create_number_button(button_container.clone(), 7);
        create_number_button(button_container.clone(), 8);
        create_operator_button(button_container.clone(), "*", CalculatorOperation::Multiply);

        create_number_button(button_container.clone(), 9);

        let decimal_rect = RectData {
            top_left: Vertex::zero(),
            width: 200,
            height: 200,
        };
        surface
            .add(
                Some(button_container.clone()),
                Button::new(
                    surface.scale_rect(decimal_rect).unwrap(),
                    decimal_rect,
                    Some(Signal::new(String::from("."))),
                    1,
                    Some(Box::new(move || {
                        let value = display_decimal.get();
                        if !value.contains('.') {
                            display_decimal.set(format!("{}.", value));
                        }
                    })),
                    Some(ComponentStylingBuilder::new().maintain_aspect_ratio(true).build()),
                ),
            )
            .unwrap();

        let sign_rect = RectData {
            top_left: Vertex::zero(),
            width: 200,
            height: 200,
        };
        surface
            .add(
                Some(button_container.clone()),
                Button::new(
                    surface.scale_rect(sign_rect).unwrap(),
                    sign_rect,
                    Some(Signal::new(String::from("±"))),
                    1,
                    Some(Box::new(move || {
                        let mut value = display_sign.get().parse::<f64>().ok().filter(|value| value.is_finite()).unwrap_or(0.0);
                        value *= -1.0;
                        display_sign.set(value.to_string());
                    })),
                    Some(ComponentStylingBuilder::new().maintain_aspect_ratio(true).build()),
                ),
            )
            .unwrap();

        create_operator_button(button_container.clone(), "/", CalculatorOperation::Divide);

        let display_clear = Rc::clone(&display_text);
        let equals_button_in_clear = Rc::clone(&equals_button);
        let stored_value_clear = Rc::clone(&stored_value);

        const CLEAR_BUTTON_COLOR: Color = Color {
            red: 255,
            green: 165,
            blue: 0,
            alpha: 255,
        };

        let clear_rect = RectData {
            top_left: Vertex::zero(),
            width: 470,
            height: 0,
        };
        surface
            .add(
                Some(action_container.clone()),
                Button::new(
                    surface.scale_rect(clear_rect).unwrap(),
                    clear_rect,
                    Some(Signal::new(String::from("C"))),
                    1,
                    Some(Box::new(move || {
                        display_clear.set(String::from("0"));

                        let mut stored = stored_value_clear.write();
                        *stored = 0.0;

                        let mut operator = current_operator.write();
                        *operator = CalculatorOperation::None;

                        if let Some(equals_button) = equals_button_in_clear.read().as_ref() {
                            if let Some(disableable) = equals_button.write().as_disableable_mut() {
                                disableable.disable();
                            }
                        }
                    })),
                    Some(
                        ComponentStylingBuilder::new()
                            .text_color(WHITE.bright())
                            .background_color(CLEAR_BUTTON_COLOR)
                            .disabled_background_color(CLEAR_BUTTON_COLOR)
                            .focused_background_color(CLEAR_BUTTON_COLOR)
                            .selected_background_color(CLEAR_BUTTON_COLOR)
                            .border_color(CLEAR_BUTTON_COLOR)
                            .disabled_border_color(CLEAR_BUTTON_COLOR)
                            .build(),
                    ),
                ),
            )
            .unwrap();

        let equals_button_init = Rc::clone(&equals_button);
        let equals_rect = RectData {
            top_left: Vertex::zero(),
            width: 470,
            height: 0,
        };
        *equals_button_init.write() = Some(
            surface
                .add(
                    Some(action_container.clone()),
                    Button::new(
                        surface.scale_rect(equals_rect).unwrap(),
                        equals_rect,
                        Some(Signal::new(String::from("="))),
                        1,
                        Some(Box::new(move || {
                            let value = equals_clone.get();
                            let stored = *equals_stored_clone.write();
                            let operator = equals_operator_clone.read().clone();

                            let current_value = value.parse::<f64>().ok().filter(|value| value.is_finite()).unwrap_or(0.0);
                            let result = match operator {
                                CalculatorOperation::Add => stored + current_value,
                                CalculatorOperation::Subtract => stored - current_value,
                                CalculatorOperation::Multiply => stored * current_value,
                                CalculatorOperation::Divide => {
                                    if current_value != 0.0 {
                                        stored / current_value
                                    } else {
                                        return;
                                    }
                                }
                                _ => return,
                            };

                            if result.is_finite() {
                                equals_clone.set(result.to_string());
                            } else {
                                equals_clone.set(String::from("Error"));
                            }
                        })),
                        None,
                    ),
                )
                .unwrap(),
        );

        let equals_button_disabling = Rc::clone(&equals_button);
        if let Some(equals_button) = equals_button_disabling.read().as_ref() {
            if let Some(disableable) = equals_button.write().as_disableable_mut() {
                disableable.disable();
            }
        };
    }
}
