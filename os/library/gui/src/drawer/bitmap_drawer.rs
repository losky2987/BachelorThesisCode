use alloc::{string::String, vec::Vec};
use drawer::{rect_data::RectData, vertex::Vertex};
use graphic::{
    bitmap::Bitmap,
    color::{BLACK, Color, INVISIBLE},
};
use libm::Libm;

pub struct BitmapDrawer<'a> {
    pub bitmap: &'a mut Bitmap,
    origin: Vertex,
}

impl<'a> BitmapDrawer<'a> {
    pub fn new(bitmap: &'a mut Bitmap) -> Self {
        Self {
            bitmap,
            origin: Vertex::zero(),
        }
    }

    pub fn new_with_origin(bitmap: &'a mut Bitmap, origin: Vertex) -> Self {
        Self { bitmap, origin }
    }

    pub fn bitmap(&self) -> &Bitmap {
        self.bitmap
    }

    pub fn bitmap_mut(&mut self) -> &mut Bitmap {
        self.bitmap
    }

    pub fn clear(&mut self, color: Color) {
        self.bitmap.clear(color);
    }

    pub fn partial_clear(&mut self, part_of_bitmap: RectData, color: Color) {
        self.draw_filled_rectangle(part_of_bitmap, color, None);
    }

    pub fn draw_pixel(&mut self, pos: Vertex, color: Color) {
        let Some(local) = self.to_local(pos) else {
            return;
        };

        self.bitmap.draw_pixel(local.x, local.y, color);
    }

    pub fn draw_line(&mut self, from: Vertex, to: Vertex, color: Color) {
        let Some(from) = self.to_local(from) else {
            return;
        };
        let Some(to) = self.to_local(to) else {
            return;
        };
        self.bitmap.draw_line(from.x, from.y, to.x, to.y, color);
    }

    pub fn draw_polygon(&mut self, vertices: Vec<Vertex>, color: Color) {
        let Some(first_vertex) = vertices.first() else {
            return;
        };

        let mut prev = first_vertex;
        for vertex in &vertices[1..] {
            self.draw_line(*prev, *vertex, color);
            prev = vertex;
        }

        self.draw_line(*prev, *first_vertex, color);
    }

    pub fn draw_rectangle(&mut self, rect_data: RectData, color: Color) {
        let RectData { top_left, width, height } = rect_data;
        let bottom_right = Vertex::new(top_left.x.saturating_add(width), top_left.y.saturating_add(height));

        self.draw_polygon(
            alloc::vec![
                Vertex::new(top_left.x, top_left.y),
                Vertex::new(bottom_right.x, top_left.y),
                Vertex::new(bottom_right.x, bottom_right.y),
                Vertex::new(top_left.x, bottom_right.y),
            ],
            color,
        );
    }

    pub fn draw_square(&mut self, top_left: Vertex, side_length: u32, color: Color) {
        self.draw_rectangle(
            RectData {
                top_left,
                width: side_length,
                height: side_length,
            },
            color,
        );
    }

    pub fn draw_filled_rectangle(&mut self, rect_data: RectData, inner_color: Color, border_color: Option<Color>) {
        let Some(rect_data) = self.to_local_rect(rect_data) else {
            return;
        };
        let RectData { top_left, width, height } = rect_data;

        match border_color {
            Some(border_color) => {
                let border_width = 3;
                self.fill_rect(top_left.x, top_left.y, width, height, border_color);

                if width > 2 * border_width && height > 2 * border_width {
                    self.fill_rect(
                        top_left.x.saturating_add(border_width),
                        top_left.y.saturating_add(border_width),
                        width - 2 * border_width,
                        height - 2 * border_width,
                        inner_color,
                    );
                }
            }
            None => self.fill_rect(top_left.x, top_left.y, width, height, inner_color),
        }
    }

    pub fn draw_filled_triangle(&mut self, vertices: [Vertex; 3], color: Color) {
        let Some(a) = self.to_local(vertices[0]) else {
            return;
        };
        let Some(b) = self.to_local(vertices[1]) else {
            return;
        };
        let Some(c) = self.to_local(vertices[2]) else {
            return;
        };
        self.fill_triangle((a.as_tuple(), b.as_tuple(), c.as_tuple()), color);
    }

    pub fn draw_circle(&mut self, center: Vertex, radius: u32, color: Color) {
        let Some(center) = self.to_local(center) else {
            return;
        };
        let max_radius = self.bitmap.width.saturating_add(self.bitmap.height).min(i32::MAX as u32);
        self.draw_circle_bresenham((center.x as i32, center.y as i32), radius.min(max_radius) as i32, color);
    }

    pub fn draw_filled_circle(&mut self, center: Vertex, radius: u32, inner_color: Color, border_color: Option<Color>) {
        let Some(local_center) = self.to_local(center) else {
            return;
        };
        let max_radius = self.bitmap.width.saturating_add(self.bitmap.height).min(i32::MAX as u32);
        let radius = radius.min(max_radius);
        self.draw_filled_circle_bresenham((local_center.x as i32, local_center.y as i32), radius as i32, inner_color);

        if let Some(border_color) = border_color {
            self.draw_circle(center, radius, border_color);
        }
    }

    pub fn draw_char(&mut self, char_to_draw: char, pos: Vertex, color: Color, scale: (u32, u32)) {
        let Some(local) = self.to_local(pos) else {
            return;
        };
        self.bitmap.draw_char_scaled(local.x, local.y, scale.0, scale.1, color, BLACK, char_to_draw);
    }

    pub fn draw_string(&mut self, string_to_draw: String, pos: Vertex, fg_color: Color, bg_color: Option<Color>, scale: (u32, u32)) {
        let Some(local) = self.to_local(pos) else {
            return;
        };
        let bg_color = bg_color.unwrap_or(INVISIBLE);
        for (index, c) in string_to_draw.chars().enumerate() {
            let Some(offset) = u32::try_from(index).ok().and_then(|index| index.checked_mul(8u32.saturating_mul(scale.0))) else {
                break;
            };
            self.bitmap
                .draw_char_scaled(local.x.saturating_add(offset), local.y, scale.0, scale.1, fg_color, bg_color, c);
        }
    }

    pub fn draw_bitmap(&mut self, pos: Vertex, bitmap: &Bitmap) {
        let Some(local) = self.to_local(pos) else {
            return;
        };
        let copy_width = bitmap.width.min(self.bitmap.width.saturating_sub(local.x)) as usize;
        let copy_height = bitmap.height.min(self.bitmap.height.saturating_sub(local.y)) as usize;
        if copy_width == 0 || copy_height == 0 {
            return;
        }

        let source_stride = bitmap.width as usize;
        let target_stride = self.bitmap.width as usize;
        if bitmap.data.len() < source_stride.saturating_mul(copy_height) {
            return;
        }
        for y in 0..copy_height {
            let source_start = y * source_stride;
            let target_start = (local.y as usize + y) * target_stride + local.x as usize;
            let source = &bitmap.data[source_start..source_start + copy_width];
            let target = &mut self.bitmap.data[target_start..target_start + copy_width];
            if source.iter().all(|color| color.alpha == 255) {
                target.copy_from_slice(source);
                continue;
            }

            for (target, color) in target.iter_mut().zip(source) {
                if color.alpha == 255 {
                    *target = *color;
                } else if color.alpha != 0 {
                    *target = target.blend(*color);
                }
            }
        }
    }

    fn to_local(&self, pos: Vertex) -> Option<Vertex> {
        if pos.x < self.origin.x || pos.y < self.origin.y {
            return None;
        }

        let local = Vertex::new(pos.x - self.origin.x, pos.y - self.origin.y);

        if local.x >= self.bitmap.width || local.y >= self.bitmap.height {
            return None;
        }

        Some(local)
    }

    fn to_local_rect(&self, rect_data: RectData) -> Option<RectData> {
        let local_top_left = self.to_local(rect_data.top_left)?;

        Some(RectData {
            top_left: local_top_left,
            width: rect_data.width,
            height: rect_data.height,
        })
    }

    fn fill_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        let end_x = x.saturating_add(width).min(self.bitmap.width);
        let end_y = y.saturating_add(height).min(self.bitmap.height);

        for draw_y in y..end_y {
            for draw_x in x..end_x {
                self.bitmap.draw_pixel(draw_x, draw_y, color);
            }
        }
    }

    fn fill_triangle(&mut self, ((mut x1, mut y1), (mut x2, mut y2), (mut x3, mut y3)): ((u32, u32), (u32, u32), (u32, u32)), color: Color) {
        if y1 > y2 {
            core::mem::swap(&mut x1, &mut x2);
            core::mem::swap(&mut y1, &mut y2);
        }
        if y1 > y3 {
            core::mem::swap(&mut x1, &mut x3);
            core::mem::swap(&mut y1, &mut y3);
        }
        if y2 > y3 {
            core::mem::swap(&mut x2, &mut x3);
            core::mem::swap(&mut y2, &mut y3);
        }

        let dx1 = if y2 != y1 { (x2 as f32 - x1 as f32) / (y2 as f32 - y1 as f32) } else { 0.0 };
        let dx2 = if y3 != y1 { (x3 as f32 - x1 as f32) / (y3 as f32 - y1 as f32) } else { 0.0 };
        let dx3 = if y3 != y2 { (x3 as f32 - x2 as f32) / (y3 as f32 - y2 as f32) } else { 0.0 };

        let mut start_x = x1 as f32;
        let mut end_x = x1 as f32;
        for y in y1..=y2 {
            self.draw_horizontal_line(start_x, end_x, y, color);
            start_x += dx1;
            end_x += dx2;
        }

        start_x = x2 as f32;
        end_x = x1 as f32 + dx2 * (y2 as f32 - y1 as f32);
        for y in y2..=y3 {
            self.draw_horizontal_line(start_x, end_x, y, color);
            start_x += dx3;
            end_x += dx2;
        }
    }

    fn draw_horizontal_line(&mut self, start_x: f32, end_x: f32, y: u32, color: Color) {
        if y >= self.bitmap.height || self.bitmap.width == 0 {
            return;
        }

        let mut start_x = Libm::<f32>::round(start_x) as u32;
        let mut end_x = Libm::<f32>::round(end_x) as u32;

        if start_x > end_x {
            core::mem::swap(&mut start_x, &mut end_x);
        }

        start_x = start_x.min(self.bitmap.width - 1);
        end_x = end_x.min(self.bitmap.width - 1);

        for x in start_x..=end_x {
            self.bitmap.draw_pixel(x, y, color);
        }
    }

    fn draw_circle_bresenham(&mut self, center: (i32, i32), radius: i32, color: Color) {
        let (cx, cy) = center;
        let mut x = 0;
        let mut y = radius;
        let mut d = 3 - 2 * radius;

        while x <= y {
            self.draw_pixel_signed(cx + x, cy + y, color);
            self.draw_pixel_signed(cx - x, cy + y, color);
            self.draw_pixel_signed(cx + x, cy - y, color);
            self.draw_pixel_signed(cx - x, cy - y, color);
            self.draw_pixel_signed(cx + y, cy + x, color);
            self.draw_pixel_signed(cx - y, cy + x, color);
            self.draw_pixel_signed(cx + y, cy - x, color);
            self.draw_pixel_signed(cx - y, cy - x, color);

            if d <= 0 {
                d += 4 * x + 6;
            } else {
                d += 4 * (x - y) + 10;
                y -= 1;
            }
            x += 1;
        }
    }

    fn draw_filled_circle_bresenham(&mut self, center: (i32, i32), radius: i32, color: Color) {
        let (cx, cy) = center;
        let mut x = 0;
        let mut y = radius;
        let mut d = 3 - 2 * radius;

        while x <= y {
            self.draw_horizontal_line_signed(cx - x, cx + x, cy + y, color);
            self.draw_horizontal_line_signed(cx - x, cx + x, cy - y, color);
            self.draw_horizontal_line_signed(cx - y, cx + y, cy + x, color);
            self.draw_horizontal_line_signed(cx - y, cx + y, cy - x, color);

            if d <= 0 {
                d += 4 * x + 6;
            } else {
                d += 4 * (x - y) + 10;
                y -= 1;
            }
            x += 1;
        }
    }

    fn draw_horizontal_line_signed(&mut self, mut start_x: i32, mut end_x: i32, y: i32, color: Color) {
        if y < 0 || y as u32 >= self.bitmap.height || self.bitmap.width == 0 {
            return;
        }
        if start_x > end_x {
            core::mem::swap(&mut start_x, &mut end_x);
        }

        let start_x = start_x.max(0) as u32;
        let end_x = end_x.min(self.bitmap.width as i32 - 1);
        if end_x < 0 || start_x > end_x as u32 {
            return;
        }

        for x in start_x..=end_x as u32 {
            self.bitmap.draw_pixel(x, y as u32, color);
        }
    }

    fn draw_pixel_signed(&mut self, x: i32, y: i32, color: Color) {
        if x < 0 || y < 0 {
            return;
        }
        self.bitmap.draw_pixel(x as u32, y as u32, color);
    }
}
