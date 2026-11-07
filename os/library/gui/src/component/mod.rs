use alloc::boxed::Box;
use drawer::rect_data::RectData;
use terminal::DecodedKey;

use crate::{component::{assistant::mouse_state::MouseEvent, container::Container}, drawer::bitmap_drawer::BitmapDrawer};

pub mod assistant;
pub mod container;
pub mod styling;
pub mod widget;

/**
Any size-relations with the words "rel" or "relative" in them refer to the size inside the window
as if the window was occupying the full screen
*/

// pub trait Component: ComponentBehaviour + Observable {}

pub trait Component: Casts {
    fn draw(&mut self, focus_id: Option<usize>, drawer: &mut BitmapDrawer);

    /// Called when the component is required to adjust its absolute bounds during the layout phase.
    /// The `parent` Container offers methods to scale relative bound to absolute bounds.
    fn rescale_to_container(&mut self, parent: &dyn Container);

    fn get_abs_rect_data(&self) -> RectData;

    fn get_drawn_rect_data(&self) -> RectData;

    fn is_dirty(&self) -> bool;

    fn get_id(&self) -> usize;

    fn mark_dirty(&mut self);
}

pub trait Disableable {
    fn is_disabled(&self) -> bool;

    fn disable(&mut self);
    
    fn enable(&mut self);
} 

pub trait Hideable {
    fn is_hidden(&self) -> bool;

    fn hide(&mut self);

    fn show(&mut self);
}

pub trait Focusable {
    fn can_unfocus(&self) -> bool { true }

    fn focus(&mut self);

    /// This will only be called if `can_unfocus()` returns true, unless the parent
    /// needs to force the unfocus NOW.
    fn unfocus(&mut self);
}

pub trait Interactable {
    fn consume_keyboard_press(&mut self, keyboard_press: DecodedKey) -> Option<Box<dyn FnOnce() -> ()>>;

    fn consume_mouse_event(&mut self, mouse_event: &MouseEvent) -> Option<Box<dyn FnOnce() -> ()>>;

    /// Whether all keyboard input must be routed to this component before the
    /// Window Manager considers global shortcuts.
    fn captures_keyboard(&self) -> bool {
        false
    }
}

pub trait Resizable {
    fn rescale(&mut self, scale_factor: f64);

    /// Resizes the component to the given abs width and height by calculating a scaling factor.
    fn resize(&mut self, width: u32, height: u32);
}

pub trait Clearable {
    fn clear(&mut self);
}

pub trait Casts {
    fn as_disableable(&self) -> Option<&dyn Disableable> {
        None
    }
    
    fn as_disableable_mut(&mut self) -> Option<&mut dyn Disableable> {
        None
    }
    
    fn as_hideable(&self) -> Option<&dyn Hideable> {
        None
    }

    fn as_hideable_mut(&mut self) -> Option<&mut dyn Hideable> {
        None
    }

    fn as_focusable(&self) -> Option<&dyn Focusable> {
        None
    }
 
    fn as_focusable_mut(&mut self) -> Option<&mut dyn Focusable> {
        None
    }
    
    fn as_interactable(&self) -> Option<&dyn Interactable> {
        None
    }
 
    fn as_interactable_mut(&mut self) -> Option<&mut dyn Interactable> {
        None
    }

    fn as_resizable(&self) -> Option<&dyn Resizable> {
        None
    }

    fn as_resizable_mut(&mut self) -> Option<&mut dyn Resizable> {
        None
    }

    fn as_clearable(&self) -> Option<&dyn Clearable> {
        None
    }

    fn as_clearable_mut(&mut self) -> Option<&mut dyn Clearable> {
        None
    }

    fn as_container(&self) -> Option<&dyn Container> {
        None
    }

    fn as_container_mut(&mut self) -> Option<&mut dyn Container> {
        None
    }
}
