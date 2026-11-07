/*

This file is a part of Library gui.

namespace: libgui::component::assistant

original name: scale.rs (gui::component::assistant::scale.rs)

author: Xiaochen Xing <xixin100@hhu.de>

created: 2026-5-27

last changed: 2026-6-24

*/

use drawer::{rect_data::RectData, vertex::Vertex};

use crate::api::screen_resolution;

/// Scales a rect (`rel_rect`) relative to a an absolute rect (`abs_container`) and returns the scaled absolute rect. \
/// The absolute rect will **always** be constrained by the absolute `min_dim` and `max_dim` sizes, disregarding the
/// aspect ratio if neccessary.
pub fn scale_rect_to_rect(rel_rect: RectData, abs_container: RectData, min_dim: (u32, u32), max_dim: (u32, u32), maintain_aspect_ratio: bool) -> RectData {
    let screen = screen_resolution();
    if screen.0 == 0 || screen.1 == 0 {
        return RectData {
            top_left: abs_container.top_left,
            width: 0,
            height: 0,
        };
    }
    let max_dim = max_dim.max(min_dim);

    // Calculate container-to-screen ratios
    let ratios = (
        f64::from(abs_container.width) / f64::from(screen.0),
        f64::from(abs_container.height) / f64::from(screen.1),
    );

    let abs_size = {
        // Scale to absolute sizes
        let abs_width = if rel_rect.width == 0 {
            abs_container.width
        } else {
            (f64::from(rel_rect.width) * ratios.0) as u32
        };

        let abs_height = if rel_rect.height == 0 {
            abs_container.height
        } else {
            (f64::from(rel_rect.height) * ratios.1) as u32
        };

        if maintain_aspect_ratio {
            // Calculate the aspect ratio while taking stretching (w/h = 0) into account
            let aspect_ratio = match (rel_rect.width, rel_rect.height) {
                (0, 0) if abs_container.height > 0 => f64::from(abs_container.width) / f64::from(abs_container.height),
                (0, 0) => 1.0,
                (0, h) => f64::from(abs_container.width) / f64::from(h),
                (w, 0) => f64::from(w) / f64::from(abs_container.height),
                (w, h) => f64::from(w) / f64::from(h),
            };

            let aspect_ratio = if aspect_ratio.is_finite() && aspect_ratio > 0.0 { aspect_ratio } else { 1.0 };
            let scaled_height = (f64::from(abs_width) / aspect_ratio) as u32;
            let scaled_width = (f64::from(abs_height) * aspect_ratio) as u32;

            // Shrink one side to maintain aspect ratio
            let scaled_size = if scaled_height <= abs_height {
                (abs_width, scaled_height)
            } else {
                (scaled_width, abs_height)
            };

            scaled_size
        } else {
            (abs_width, abs_height)
        }
    };

    RectData {
        top_left: Vertex::new(
            ((f64::from(rel_rect.top_left.x) * ratios.0) as u32).saturating_add(abs_container.top_left.x),
            ((f64::from(rel_rect.top_left.y) * ratios.1) as u32).saturating_add(abs_container.top_left.y),
        ),
        width: abs_size.0.clamp(min_dim.0, max_dim.0),
        height: abs_size.1.clamp(min_dim.1, max_dim.1),
    }
}

/// Scales a radius to be relative to an absolute rect.
pub fn scale_radius_to_rect(rel_radius: u32, min_radius: u32, abs_rect: RectData) -> u32 {
    let screen = screen_resolution();
    if screen.0 == 0 || screen.1 == 0 {
        return min_radius;
    }

    let ratios = (
        f64::from(abs_rect.width) / f64::from(screen.0),
        f64::from(abs_rect.height) / f64::from(screen.1),
    );

    let scaled_radius = (f64::from(rel_radius) * ratios.0.min(ratios.1)) as u32;

    scaled_radius.max(min_radius)
}

/// Scales a position relative to an absolute rect and returns the absolute position.
pub fn scale_pos_to_rect(rel_pos: Vertex, abs_rect: RectData) -> Vertex {
    let screen = screen_resolution();
    if screen.0 == 0 || screen.1 == 0 {
        return abs_rect.top_left;
    }

    let ratios = (
        f64::from(abs_rect.width) / f64::from(screen.0),
        f64::from(abs_rect.height) / f64::from(screen.1),
    );

    Vertex::new(
        ((f64::from(rel_pos.x) * ratios.0) as u32).saturating_add(abs_rect.top_left.x),
        ((f64::from(rel_pos.y) * ratios.1) as u32).saturating_add(abs_rect.top_left.y),
    )
}
