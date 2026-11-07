use alloc::collections::linked_list::CursorMut;
use core::sync::atomic::{AtomicUsize, Ordering};

use alloc::collections::LinkedList;
use drawer::rect_data::RectData;
use drawer::vertex::Vertex;
use hashbrown::HashMap;

use crate::window::app::AppWindow;
use crate::window::window_tree::WindowNode;

static WORKSPACE_ID_COUNTER: AtomicUsize = AtomicUsize::new(1);

/**
A workspace is a unit of one screen, containing windows. You can switch between workspaces
and they will retain their state and even continue execution of their threads, but not draw
anything to the screen while not selected of course.
*/
pub struct Workspace {
    id: usize,

    pub windows: HashMap<usize, AppWindow>,
    pub focused_window_id: usize,
    // Windows are stored additionally in ordered fashion in here
    pub window_orderer: LinkedList<usize>,
    /**
    Proxy-structure to determine buddies to be merged together
    */
    buddy_tree_root: WindowNode,
}

impl Workspace {
    pub fn new_with_single_window(window: (usize, AppWindow), focused_window_id: usize) -> Self {
        let mut window_orderer = LinkedList::new();
        window_orderer.push_back(window.0);

        let mut windows: HashMap<usize, AppWindow> = HashMap::new();
        windows.insert(window.0, window.1);

        let buddy_tree_root = WindowNode::new_leaf(window.0);

        Self {
            id: WORKSPACE_ID_COUNTER.fetch_add(1, Ordering::SeqCst),
            windows,
            focused_window_id,
            window_orderer,
            buddy_tree_root,
        }
    }

    pub fn insert_window(&mut self, window: AppWindow, after: usize) {
        let new_window_id = window.get_id();
        self.windows.insert(new_window_id, window);

        self.buddy_tree_root.insert_value(after, new_window_id);

        let mut cursor = get_element_cursor_from_orderer(&mut self.window_orderer, after).unwrap();
        cursor.move_next();
        cursor.insert_before(new_window_id);
    }

    pub fn focus_next_window(&mut self) {
        if !self.can_unfocus_current_window() {
            return;
        }

        self.unfocus_current_window();
        self.get_focused_window_mut().mark_window_dirty(); // old window

        let mut cursor = get_element_cursor_from_orderer(&mut self.window_orderer, self.focused_window_id).unwrap();

        cursor.move_next();

        self.focused_window_id = match cursor.current() {
            Some(next_focused_el) => next_focused_el.clone(),
            None => {
                cursor.move_next();
                cursor.current().unwrap().clone()
            }
        };

        self.get_focused_window_mut().mark_window_dirty(); // new window
    }

    pub fn focus_prev_window(&mut self) {
        if !self.can_unfocus_current_window() {
            return;
        }

        self.unfocus_current_window();
        self.get_focused_window_mut().mark_window_dirty(); // old window

        let mut cursor = get_element_cursor_from_orderer(&mut self.window_orderer, self.focused_window_id).unwrap();

        cursor.move_prev();

        self.focused_window_id = match cursor.current() {
            Some(next_focused_el) => next_focused_el.clone(),
            None => {
                cursor.move_prev();
                cursor.current().unwrap().clone()
            }
        };

        self.get_focused_window_mut().mark_window_dirty(); // new window
    }

    pub fn focus_window_at(&mut self, pos: Vertex) {
        if !self.can_unfocus_current_window() {
            return;
        }

        // Find the window that contains the position
        let new_window_id = self
            .window_orderer
            .iter()
            .find(|id| self.windows.get(*id).unwrap().rect_data.contains_vertex(&pos))
            .copied();

        if let Some(new_window_id) = new_window_id {
            if self.focused_window_id == new_window_id {
                return;
            }

            // Mark the old window as dirty
            self.unfocus_current_window();
            self.get_focused_window_mut().mark_window_dirty();

            self.focused_window_id = new_window_id;

            // Mark the new window as dirty
            self.get_focused_window_mut().mark_window_dirty();
        }
    }

    /// Moves focus to the next component in currently focused window
    pub fn focus_next_component(&mut self) {
        self.get_focused_window_mut().focus_next_app_component();
    }

    /// Moves focus to the previous component in currently focused window
    pub fn focus_prev_component(&mut self) {
        self.get_focused_window_mut().focus_prev_app_component();
    }

    // Focus a component at the given position in the currently focused window
    pub fn focus_component_at(&mut self, pos: Vertex) {
        let _ = pos;
    }

    /// Remove a window from the layout after the Window Manager has handled
    /// the application lifecycle for that window.
    pub fn remove_target_window(&mut self, window_id: usize) -> bool {
        if self.windows.len() == 1 || !self.windows.contains_key(&window_id) {
            return false;
        }

        let Some(sibling_ids) = self.buddy_tree_root.get_sibling_leaves(window_id) else {
            return false;
        };
        if sibling_ids.is_empty()
            || !self.window_orderer.iter().any(|ordered_window_id| *ordered_window_id == window_id)
            || sibling_ids.iter().any(|sibling_id| !self.windows.contains_key(sibling_id))
        {
            return false;
        }
        let mut updated_tree = self.buddy_tree_root.clone();
        if !updated_tree.remove_leaf(window_id) {
            return false;
        }

        let removed_rect = self.windows.get(&window_id).unwrap().rect_data.clone();
        let mut sibling_bounds = self.windows.get(&sibling_ids[0]).unwrap().rect_data.clone();
        for sibling_id in sibling_ids.iter().skip(1) {
            sibling_bounds = rect_union(sibling_bounds, self.windows.get(sibling_id).unwrap().rect_data.clone());
        }
        let expanded_bounds = rect_union(removed_rect, sibling_bounds);

        for sibling_id in sibling_ids {
            let window = self.windows.get_mut(&sibling_id).unwrap();
            let expanded_rect = scale_rect(window.rect_data.clone(), sibling_bounds, expanded_bounds);
            window.rescale_window_in_place(expanded_rect);
            window.resize_app_surface(window.content_rect());
        }

        self.windows.remove(&window_id);
        self.buddy_tree_root = updated_tree;

        let target_was_focused = self.focused_window_id == window_id;
        let Some(mut cursor) = get_element_cursor_from_orderer(&mut self.window_orderer, window_id) else {
            return false;
        };
        cursor.move_next();

        let new_focused_window_id = if target_was_focused {
            match cursor.current() {
                Some(next_focused_el) => next_focused_el.clone(),
                None => cursor.peek_next().unwrap().clone(),
            }
        } else {
            self.focused_window_id
        };

        cursor.move_prev();
        cursor.remove_current();

        self.focused_window_id = new_focused_window_id;

        true
    }

    pub fn move_focused_window_forward(&mut self) {
        if self.windows.len() == 1 {
            return;
        }

        let focused_rect_data = {
            let focused_window = self.get_focused_window_mut();
            let focused_rect_data = focused_window.rect_data.clone();

            focused_rect_data
        };

        let swapped_rect_data = {
            let mut cursor = get_element_cursor_from_orderer(&mut self.window_orderer, self.focused_window_id).unwrap();

            let focused = cursor.remove_current().unwrap();

            match cursor.current() {
                Some(_) => cursor.insert_after(focused),
                None => {
                    cursor.move_next();
                    cursor.insert_after(focused);
                }
            }

            let swapped_id = cursor.current().unwrap().clone();

            let swapped_window = self.windows.get_mut(&swapped_id).unwrap();
            let swapped_rect_data: RectData = swapped_window.rect_data.clone();
            swapped_window.rescale_window_after_move(focused_rect_data);
            swapped_window.resize_app_surface(swapped_window.content_rect());

            self.buddy_tree_root.swap_values(self.focused_window_id, swapped_id);

            swapped_rect_data
        };

        let focused_window_id = self.focused_window_id;
        let focused_content_rect = {
            let focused_window = self.get_focused_window_mut();
            focused_window.rect_data = swapped_rect_data;
            focused_window.rescale_window_after_move(swapped_rect_data);
            focused_window.content_rect()
        };
        if let Some(window) = self.windows.get_mut(&focused_window_id) {
            window.resize_app_surface(focused_content_rect);
        }
    }

    pub fn move_focused_window_backward(&mut self) {
        if self.windows.len() == 1 {
            return;
        }

        let focused_rect_data = {
            let focused_window = self.get_focused_window_mut();
            let focused_rect_data = focused_window.rect_data.clone();

            focused_rect_data
        };

        let swapped_rect_data = {
            let mut cursor = get_element_cursor_from_orderer(&mut self.window_orderer, self.focused_window_id).unwrap();

            let focused = cursor.remove_current().unwrap();
            cursor.move_prev();

            match cursor.current() {
                Some(_) => cursor.insert_before(focused),
                None => {
                    cursor.move_prev();
                    cursor.insert_before(focused);
                }
            }

            let swapped_id = cursor.current().unwrap().clone();

            let swapped_window = self.windows.get_mut(&swapped_id).unwrap();
            let swapped_rect_data = swapped_window.rect_data.clone();
            swapped_window.rescale_window_after_move(focused_rect_data);
            swapped_window.resize_app_surface(swapped_window.content_rect());

            self.buddy_tree_root.swap_values(self.focused_window_id, swapped_id);

            swapped_rect_data
        };

        let focused_window_id = self.focused_window_id;
        let focused_content_rect = {
            let focused_window = self.get_focused_window_mut();
            focused_window.rect_data = swapped_rect_data;
            focused_window.rescale_window_after_move(swapped_rect_data);
            focused_window.content_rect()
        };
        if let Some(window) = self.windows.get_mut(&focused_window_id) {
            window.resize_app_surface(focused_content_rect);
        }
    }

    fn can_unfocus_current_window(&self) -> bool {
        if !self.get_focused_window().can_unfocus() {
            return false;
        }

        true
    }

    fn unfocus_current_window(&mut self) {
        let window = self.get_focused_window_mut();
        window.unfocus();
        window.unfocus_app_surface();
    }

    #[allow(dead_code)]
    pub fn get_focused_window(&self) -> &AppWindow {
        self.windows.get(&self.focused_window_id).unwrap()
    }

    pub fn get_focused_window_mut(&mut self) -> &mut AppWindow {
        self.windows.get_mut(&self.focused_window_id).unwrap()
    }

    pub fn get_id(&self) -> usize {
        self.id
    }
}

pub fn get_element_cursor_from_orderer<T: PartialEq>(linked_list: &mut LinkedList<T>, needle: T) -> Option<CursorMut<'_, T>> {
    let mut cursor = linked_list.cursor_front_mut();
    while let Some(element) = cursor.current() {
        if *element == needle {
            return Some(cursor);
        }

        cursor.move_next();
    }

    return None;
}

fn rect_union(first: RectData, second: RectData) -> RectData {
    let top_left = Vertex::new(
        first.top_left.x.min(second.top_left.x),
        first.top_left.y.min(second.top_left.y),
    );
    let right = first
        .top_left
        .x
        .saturating_add(first.width)
        .max(second.top_left.x.saturating_add(second.width));
    let bottom = first
        .top_left
        .y
        .saturating_add(first.height)
        .max(second.top_left.y.saturating_add(second.height));

    RectData {
        top_left,
        width: right.saturating_sub(top_left.x),
        height: bottom.saturating_sub(top_left.y),
    }
}

fn scale_rect(rect: RectData, source: RectData, target: RectData) -> RectData {
    let source_right = rect.top_left.x.saturating_add(rect.width);
    let source_bottom = rect.top_left.y.saturating_add(rect.height);
    let target_left = scale_edge(
        rect.top_left.x,
        source.top_left.x,
        source.width,
        target.top_left.x,
        target.width,
    );
    let target_top = scale_edge(
        rect.top_left.y,
        source.top_left.y,
        source.height,
        target.top_left.y,
        target.height,
    );
    let target_right = scale_edge(
        source_right,
        source.top_left.x,
        source.width,
        target.top_left.x,
        target.width,
    );
    let target_bottom = scale_edge(
        source_bottom,
        source.top_left.y,
        source.height,
        target.top_left.y,
        target.height,
    );

    RectData {
        top_left: Vertex::new(target_left, target_top),
        width: target_right.saturating_sub(target_left),
        height: target_bottom.saturating_sub(target_top),
    }
}

fn scale_edge(
    value: u32,
    source_start: u32,
    source_extent: u32,
    target_start: u32,
    target_extent: u32,
) -> u32 {
    if source_extent == 0 {
        return target_start;
    }

    let source_offset = value.saturating_sub(source_start) as u64;
    let scaled_offset = source_offset.saturating_mul(target_extent as u64) / source_extent as u64;
    target_start.saturating_add(scaled_offset.min(u32::MAX as u64) as u32)
}
