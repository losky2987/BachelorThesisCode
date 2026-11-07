#![no_std]

extern crate alloc;

mod text_editor;

use gui::{
    api::{ComponentFrameHandle, LOG_SCREEN, component_ref, current_component_frame}, component::{
        assistant::{config::DEFAULT_BACKGROUND_COLOR, signal::{ComponentRef, Signal}},
        container::{
            basic::BasicContainer,
            layout::{AlignmentMode, ContainerLayoutBuilder, HorizontalDirection, StretchMode, VerticalDirection},
            styling::ContainerStyling,
        },
        styling::ComponentStyling,
        widget::{button::Button, canvas::Canvas, label::Label},
    }, runtime::{executable::GAppExecutable, gapp_bootloader}
};

use crate::text_editor::{config::TextEditorConfig, editor::{OpenDocuments, apply_message}, view::View, messages::{Message, CommandMessage}};

use alloc::{boxed::Box, rc::Rc, string::String, vec};
use drawer::{rect_data::RectData, vertex::Vertex};
use graphic::bitmap::{Bitmap, ScalingMode};
use spin::rwlock::RwLock;
use terminal::DecodedKey;

#[unsafe(no_mangle)]
pub fn main() {
    gapp_bootloader::run(TextEditor::run);
}

// Julius Drodofsky

pub struct TextEditor;

impl GAppExecutable for TextEditor {
    fn run() {
        let surface = current_component_frame().expect("ComponentFrame not registered for app thread");
        let config =
            TextEditorConfig::new(LOG_SCREEN.0 as usize - 100, LOG_SCREEN.1 as usize - 40, &[]);
        let bitmap = Bitmap {
            width: config.width as u32,
            height: config.height as u32,
            data: vec![config.background_color; config.width * config.height],
        };

        let canvas = Rc::new(RwLock::new(bitmap));
        let canvs_clone = Rc::clone(&canvas);
        let edit_canvas: Rc<RwLock<Option<ComponentRef>>> = Rc::new(RwLock::new(None));
        let mut documents = OpenDocuments::dummy();
        View::render(documents.current().unwrap(), &mut canvas.write());
        let mut container_styling = ContainerStyling::default();
        let curret_file = Signal::new(documents.current().unwrap().path().unwrap());
        container_styling.show_border = false;
        container_styling.maintain_aspect_ratio = false;
        container_styling.child_padding = 2;
        container_styling.background_color = DEFAULT_BACKGROUND_COLOR;
        container_styling.show_background = true;
        let label_styling = ComponentStyling::default();
        let model = Rc::new(RwLock::<OpenDocuments<'_, '_>>::new(documents));

        fn create_container(
            surface: &ComponentFrameHandle,
            parent: Option<ComponentRef>,
            log_rect_data: RectData,
            layout: Option<gui::component::container::layout::ContainerLayout>,
            styling: Option<ContainerStyling>,
        ) -> ComponentRef {
            surface.add_deferred(
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
            log_rect_data: RectData,
            label: &str,
            font_size: usize,
            on_click: Box<dyn Fn() -> ()>,
        ) -> ComponentRef {
            surface.add_deferred(
                parent,
                Button::new(
                    surface.scale_rect(log_rect_data).unwrap(),
                    log_rect_data,
                    Some(Signal::new(String::from(label))),
                    font_size,
                    Some(on_click),
                    None,
                ),
            ).expect("failed to create button")
        }

        let parent_container = create_container(
            &surface,
            None,
            RectData {
                top_left: Vertex { x: 50, y: 50 },
                width: LOG_SCREEN.0 - 100,
                height: LOG_SCREEN.1 - 140,
            },
            Some(
                ContainerLayoutBuilder::new()
                    .alignment(AlignmentMode::Vertical(VerticalDirection::Top))
                    .stretch(StretchMode::Fill)
                    .build(),
            ),
            Some(container_styling),
        );

        let menu_container = create_container(
            &surface,
            Some(parent_container.clone()),
            RectData {
                top_left: Vertex { x: 0, y: 0 },
                width: LOG_SCREEN.0 as u32 - 100,
                height: 40,
            },
            Some(
                ContainerLayoutBuilder::new()
                    .alignment(AlignmentMode::Horizontal(HorizontalDirection::Left))
                    .stretch(StretchMode::Fill)
                    .build(),
            ),
            Some(container_styling),
        );

        surface.add_deferred(Some(menu_container.clone()), Label::new(
            surface.scale_vertex(Vertex::new(0, 0)).unwrap(),
            1,
            Rc::clone(&curret_file),
            Some(label_styling),
        )).unwrap();

        let model_clone = Rc::clone(&model);
        let canvas_clone = Rc::clone(&canvas);
        let edit_canvas_clone = Rc::clone(&edit_canvas);
        create_button(
            &surface,
            Some(menu_container.clone()),
            RectData { top_left: Vertex::new(0, 0), width: 100, height: 60 },
            "Undo",
            0,
            Box::new(move || {
                apply_message(
                    &model_clone,
                    &Rc::clone(&canvas_clone),
                    Message::CommandMessage(CommandMessage::Undo),
                );
                match edit_canvas_clone.write().as_ref() {
                    Some(c) => c.write().mark_dirty(),
                    None => (),
                }
            }),
        );

        let model_clone = Rc::clone(&model);
        let canvas_clone = Rc::clone(&canvas);
        let edit_canvas_clone = Rc::clone(&edit_canvas);
        create_button(
            &surface,
            Some(menu_container.clone()),
            RectData { top_left: Vertex::new(0, 0), width: 100, height: 60 },
            "Redo",
            1,
            Box::new(move || {
                apply_message(
                    &model_clone,
                    &Rc::clone(&canvas_clone),
                    Message::CommandMessage(CommandMessage::Redo),
                );
                match edit_canvas_clone.write().as_ref() {
                    Some(c) => c.write().mark_dirty(),
                    None => (),
                }
            }),
        );

        let model_clone = Rc::clone(&model);
        let canvas_clone = Rc::clone(&canvas);
        let edit_canvas_clone = Rc::clone(&edit_canvas);
        create_button(
            &surface,
            Some(menu_container.clone()),
            RectData { top_left: Vertex::new(0, 0), width: 240, height: 60 },
            "MD - Preview",
            1,
            Box::new(move || {
                apply_message(
                    &model_clone,
                    &Rc::clone(&canvas_clone),
                    Message::CommandMessage(CommandMessage::Markdown),
                );
                match edit_canvas_clone.write().as_ref() {
                    Some(c) => c.write().mark_dirty(),
                    None => (),
                }
            }),
        );

        let model_clone = Rc::clone(&model);
        let canvas_clone = Rc::clone(&canvas);
        let edit_canvas_clone = Rc::clone(&edit_canvas);
        create_button(
            &surface,
            Some(menu_container.clone()),
            RectData { top_left: Vertex::new(0, 0), width: 140, height: 60 },
            "Code",
            1,
            Box::new(move || {
                apply_message(
                    &model_clone,
                    &Rc::clone(&canvas_clone),
                    Message::CommandMessage(CommandMessage::CLike),
                );
                match edit_canvas_clone.write().as_ref() {
                    Some(c) => c.write().mark_dirty(),
                    None => (),
                }
            }),
        );

        let model_clone = Rc::clone(&model);
        let canvas_clone = Rc::clone(&canvas);
        let edit_canvas_clone = Rc::clone(&edit_canvas);
        let current_file_clone = Rc::clone(&curret_file);
        create_button(
            &surface,
            Some(menu_container.clone()),
            RectData { top_left: Vertex::new(0, 0), width: 60, height: 60 },
            "<",
            1,
            Box::new(move || {
                {
                    let mut models = model_clone.write();
                    let prev = match models.prev() {
                        Some(p) => p,
                        None => return,
                    };
                    current_file_clone.set(prev.path().unwrap_or(String::from("scratch")));
                }
                apply_message(
                    &model_clone,
                    &Rc::clone(&canvas_clone),
                    Message::CommandMessage(CommandMessage::None),
                );
                match edit_canvas_clone.write().as_ref() {
                    Some(c) => c.write().mark_dirty(),
                    None => (),
                }
            }),
        );

        let model_clone = Rc::clone(&model);
        let canvas_clone = Rc::clone(&canvas);
        let edit_canvas_clone = Rc::clone(&edit_canvas);
        let current_file_clone = Rc::clone(&curret_file);
        create_button(
            &surface,
            Some(menu_container.clone()),
            RectData { top_left: Vertex::new(0, 0), width: 60, height: 60 },
            ">",
            1,
            Box::new(move || {
                {
                    let mut models = model_clone.write();
                    let next = match models.next() {
                        Some(n) => n,
                        None => return,
                    };
                    current_file_clone.set(next.path().unwrap_or(String::from("scratch")));
                }
                apply_message(
                    &model_clone,
                    &Rc::clone(&canvas_clone),
                    Message::CommandMessage(CommandMessage::None),
                );
                match edit_canvas_clone.write().as_ref() {
                    Some(c) => c.write().mark_dirty(),
                    None => (),
                }
            }),
        );

        let canvas_rect = RectData {
            top_left: Vertex::new(0, 0),
            width: config.width as u32,
            height: config.height as u32,
        };
        *edit_canvas.write() = Some(surface.add_deferred(
            Some(parent_container.clone()),
            component_ref(Canvas::new(
                None,
                canvas_rect,
                surface.scale_rect(canvas_rect).unwrap(),
                Rc::clone(&canvas),
                ScalingMode::None,
                Some(Box::new(move |c: DecodedKey| {
                    apply_message(&model, &canvs_clone, Message::DecodedKey(c));
                })),
            )),
        ).unwrap());

        surface.update_frame().unwrap();
    }
}
