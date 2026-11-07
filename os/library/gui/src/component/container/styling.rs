/*

This file is a part of Library gui.

Header file of libgui::component::container::styling <=> styling.h + styling.impl.c

original name: styling.rs (gui::component::container::styling.rs)

author: Xiaochen Xing <xixin100@hhu.de>

created: 2026-5-27

last changed: 2026-6-24

*/

use graphic::color::Color;

use crate::component::assistant::config::{DEFAULT_BACKGROUND_COLOR, DEFAULT_BORDER_COLOR};

#[derive(Clone, Copy)]
pub struct ContainerStyling {
    pub maintain_aspect_ratio: bool,
    pub show_border: bool,
    pub show_background: bool,
    pub child_padding: u32,

    pub border_color: Color,
    pub background_color: Color,
}

impl Default for ContainerStyling {
    fn default() -> Self {
        Self {
            maintain_aspect_ratio: false,
            show_border: true,
            show_background: false,
            child_padding: 5,

            border_color: DEFAULT_BORDER_COLOR,
            background_color: DEFAULT_BACKGROUND_COLOR,
        }
    }
}

pub struct ContainerStylingBuilder {
    maintain_aspect_ratio: Option<bool>,
    show_border: Option<bool>,
    show_background: Option<bool>,
    child_padding: Option<u32>,

    border_color: Option<Color>,
    background_color: Option<Color>,
}

impl ContainerStylingBuilder {
    pub fn new() -> Self {
        Self {
            maintain_aspect_ratio: None,
            show_border: None,
            show_background: None,
            child_padding: None,

            border_color: None,
            background_color: None,
        }
    }

    pub fn maintain_aspect_ratio(&mut self, maintain_aspect_ratio: bool) -> &mut Self {
        self.maintain_aspect_ratio = Some(maintain_aspect_ratio);
        self
    }

    pub fn show_border(&mut self, show_border: bool) -> &mut Self {
        self.show_border = Some(show_border);
        self
    }

    pub fn show_background(&mut self, show_background: bool) -> &mut Self {
        self.show_background = Some(show_background);
        self
    }

    pub fn child_padding(&mut self, child_padding: u32) -> &mut Self {
        self.child_padding = Some(child_padding);
        self
    }

    pub fn border_color(&mut self, color: Color) -> &mut Self {
        self.border_color = Some(color);
        self
    }

    pub fn background_color(&mut self, color: Color) -> &mut Self {
        self.background_color = Some(color);
        self
    }

    pub fn build(&mut self) -> ContainerStyling {
        ContainerStyling {
            maintain_aspect_ratio: self.maintain_aspect_ratio.unwrap_or(false),
            show_border: self.show_border.unwrap_or(true),
            show_background: self.show_background.unwrap_or(false),
            child_padding: self.child_padding.unwrap_or(5),

            border_color: self.border_color.unwrap_or(DEFAULT_BORDER_COLOR),
            background_color: self.background_color.unwrap_or(DEFAULT_BACKGROUND_COLOR),
        }
    }
}
