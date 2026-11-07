#![no_std]

extern crate alloc;

use alloc::vec;
use drawer::{rect_data::RectData, vertex::Vertex};
use graphic::{bitmap::{Bitmap, ScalingMode}, color::Color};

use gui::{
    api::{component_ref, current_component_frame}, component::{styling::ComponentStylingBuilder, widget::bitmap::BitmapGraphic}, runtime::{executable::GAppExecutable, gapp_bootloader::{self}}
};

#[unsafe(no_mangle)]
pub fn main() {
    gapp_bootloader::run(BitmapApp::run);
}

pub struct BitmapApp;

impl GAppExecutable for BitmapApp {
    fn run() {
        let surface = current_component_frame().expect("ComponentFrame not registered for app thread");

        let bitmap_red = Bitmap {
            width: 10,
            height: 10,
            data: vec![
                Color { red: 255, green: 0, blue: 0, alpha: 255 };
                100
            ],
        };

        let checkerboard_bitmap = Bitmap {
            width: 10,
            height: 10,
            data: (0..100)
                .map(|i| {
                    if (i / 10 + i % 10) % 2 == 0 {
                        Color { red: 255, green: 255, blue: 255, alpha: 255 }
                    } else {
                        Color { red: 0, green: 0, blue: 0, alpha: 255 }
                    }
                })
                .collect(),
        };

        let transparent_bitmap = Bitmap {
            width: 5,
            height: 5,
            data: vec![
                Color { red: 255, green: 0, blue: 0, alpha: 255 },
                Color { red: 0, green: 255, blue: 0, alpha: 192 },
                Color { red: 0, green: 0, blue: 255, alpha: 128 },
                Color { red: 255, green: 255, blue: 0, alpha: 64 },
                Color { red: 0, green: 0, blue: 0, alpha: 0 },

                Color { red: 255, green: 0, blue: 0, alpha: 255 },
                Color { red: 0, green: 255, blue: 0, alpha: 192 },
                Color { red: 0, green: 0, blue: 255, alpha: 128 },
                Color { red: 255, green: 255, blue: 0, alpha: 64 },
                Color { red: 0, green: 0, blue: 0, alpha: 0 },

                Color { red: 255, green: 0, blue: 0, alpha: 255 },
                Color { red: 0, green: 255, blue: 0, alpha: 192 },
                Color { red: 0, green: 0, blue: 255, alpha: 128 },
                Color { red: 255, green: 255, blue: 0, alpha: 64 },
                Color { red: 0, green: 0, blue: 0, alpha: 0 },

                Color { red: 255, green: 0, blue: 0, alpha: 255 },
                Color { red: 0, green: 255, blue: 0, alpha: 192 },
                Color { red: 0, green: 0, blue: 255, alpha: 128 },
                Color { red: 255, green: 255, blue: 0, alpha: 64 },
                Color { red: 0, green: 0, blue: 0, alpha: 0 },

                Color { red: 255, green: 0, blue: 0, alpha: 255 },
                Color { red: 0, green: 255, blue: 0, alpha: 192 },
                Color { red: 0, green: 0, blue: 255, alpha: 128 },
                Color { red: 255, green: 255, blue: 0, alpha: 64 },
                Color { red: 0, green: 0, blue: 0, alpha: 0 },
            ],
        };

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

        fn add_bitmap(surface: &gui::api::ComponentFrameHandle, log_rect: RectData, bitmap: Bitmap, scaling_mode: ScalingMode, styling: Option<gui::component::styling::ComponentStyling>) {
            surface.add(None, component_ref(BitmapGraphic::new(
                surface.scale_rect(log_rect).unwrap(),
                log_rect,
                bitmap,
                scaling_mode,
                styling,
            ))).unwrap();
        }

        add_bitmap(
            &surface,
            RectData { top_left: Vertex::new(50, 50), width: 50, height: 50 },
            bitmap_red,
            ScalingMode::NearestNeighbor,
            None,
        );

        add_bitmap(
            &surface,
            RectData { top_left: Vertex::new(150, 50), width: 50, height: 50 },
            checkerboard_bitmap,
            ScalingMode::NearestNeighbor,
            None,
        );

        add_bitmap(
            &surface,
            RectData { top_left: Vertex::new(50, 150), width: 50, height: 50 },
            transparent_bitmap,
            ScalingMode::NearestNeighbor,
            Some(ComponentStylingBuilder::new()
                .maintain_aspect_ratio(true)
                .build()),
        );

        add_bitmap(
            &surface,
            RectData { top_left: Vertex::new(150, 150), width: 200, height: 100 },
            gradient_bitmap,
            ScalingMode::Bilinear,
            Some(ComponentStylingBuilder::new()
                .maintain_aspect_ratio(true)
                .build()),
        );
    }
}
