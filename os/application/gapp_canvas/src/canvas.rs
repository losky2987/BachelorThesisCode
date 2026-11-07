#![no_std]

extern crate alloc;

use alloc::collections::vec_deque::VecDeque;
use alloc::{boxed::Box, rc::Rc, vec};
use drawer::{rect_data::RectData, vertex::Vertex};
use graphic::color::Color;
use graphic::lfb::DEFAULT_CHAR_WIDTH;
use graphic::{
    bitmap::{Bitmap, ScalingMode},
    lfb::DEFAULT_CHAR_HEIGHT,
};
use gui::runtime::gapp_bootloader;
use spin::rwlock::RwLock;
use terminal::DecodedKey;

use gui::{
    api::{component_ref, current_component_frame},
    component::widget::canvas::Canvas,
    runtime::executable::GAppExecutable,
};

#[unsafe(no_mangle)]
pub fn main() {
    gapp_bootloader::run(CanvasApp::run);
}

// Julius Drodofsky
pub struct CanvasApp;

fn render_input(key: DecodedKey, canvas: &Rc<RwLock<Bitmap>>, location: &Rc<RwLock<Vertex>>) {
    let mut position: Vertex;
    {
        position = *location.read();
    }

    let key = match key {
        DecodedKey::RawKey(_) => {
            return;
        }
        DecodedKey::Unicode(u) => u,
    };
    if key == '\n' {
        position.y = position.y.saturating_add(DEFAULT_CHAR_HEIGHT);
        position.x = 0;
        *location.write() = position;
        return;
    }
    if canvas.read().width.saturating_sub(position.x) < DEFAULT_CHAR_WIDTH {
        position.x = 0;
        position.y = position.y.saturating_add(DEFAULT_CHAR_HEIGHT);
    }
    let advance = canvas
        .write()
        .draw_char_scaled(position.x, position.y, 1, 1, Color::new(255, 255, 255, 255), Color::new(0, 0, 0, 50), key);
    position.x = position.x.saturating_add(advance);
    *location.write() = position;
}

impl GAppExecutable for CanvasApp {
    fn run() {
        let surface = current_component_frame().expect("ComponentFrame not registered for app thread");

        let bitmap_red = Bitmap {
            width: 200,
            height: 100,
            data: vec![
                Color {
                    red: 255,
                    green: 0,
                    blue: 0,
                    alpha: 255
                };
                20000
            ],
        };
        let deque = VecDeque::<DecodedKey>::new();
        let mut position_v = Vertex::zero();
        position_v.y = DEFAULT_CHAR_HEIGHT;
        let canvas = Rc::new(RwLock::new(bitmap_red));
        let position = Rc::new(RwLock::new(position_v));
        let _input = Rc::new(RwLock::<VecDeque<DecodedKey>>::new(deque));

        let mut x = 0;
        x = canvas
            .write()
            .draw_char_scaled(x, 0, 1, 1, Color::new(255, 255, 255, 255), Color::new(0, 0, 0, 50), 'R');
        canvas
            .write()
            .draw_char_scaled(x, 0, 1, 1, Color::new(255, 255, 255, 255), Color::new(0, 0, 0, 50), 'o');
        canvas
            .write()
            .draw_char_scaled(x * 2, 0, 1, 1, Color::new(255, 255, 255, 255), Color::new(0, 0, 0, 50), 't');
        canvas
            .write()
            .draw_line(0, DEFAULT_CHAR_HEIGHT, x * 3, DEFAULT_CHAR_HEIGHT, Color::new(255, 255, 255, 255));

        let canvas_rect = RectData {
            top_left: Vertex::new(50, 80),
            width: 200,
            height: 100,
        };
        surface
            .add(
                None,
                component_ref(Canvas::new(
                    None,
                    canvas_rect,
                    surface.scale_rect(canvas_rect).unwrap(),
                    Rc::clone(&canvas),
                    ScalingMode::NearestNeighbor,
                    Some(Box::new(move |c: DecodedKey| {
                        render_input(c, &canvas, &position);
                    })),
                )),
            )
            .unwrap();
    }
}
