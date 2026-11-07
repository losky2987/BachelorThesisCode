use alloc::vec;
use drawer::rect_data::RectData;
use graphic::{bitmap::Bitmap, color::INVISIBLE};

use crate::{component::assistant::signal::ComponentRef, drawer::bitmap_drawer::BitmapDrawer};

pub mod bitmap_drawer;
pub mod component_tree;

pub fn draw_component_frame(root: ComponentRef, frame_rect: RectData, focus_id: Option<usize>) -> Bitmap {
    let pixel_count = (frame_rect.width as usize).checked_mul(frame_rect.height as usize).unwrap_or(0);
    let mut bitmap = Bitmap {
        width: frame_rect.width,
        height: frame_rect.height,
        data: vec![INVISIBLE; pixel_count],
    };

    let mut drawer = BitmapDrawer::new_with_origin(&mut bitmap, frame_rect.top_left);

    root.write().draw(focus_id, &mut drawer);

    bitmap
}
