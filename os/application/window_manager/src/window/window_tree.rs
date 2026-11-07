use alloc::{boxed::Box, vec::Vec};

/// Used to determine and manage buddy-IDs of AppWindows
#[derive(Clone)]
pub enum WindowNode {
    Leaf(usize),
    Branch { left: Box<WindowNode>, right: Box<WindowNode> },
}

impl WindowNode {
    pub fn new_leaf(value: usize) -> Self {
        WindowNode::Leaf(value)
    }

    pub fn new_branch(left: WindowNode, right: WindowNode) -> Self {
        WindowNode::Branch {
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    pub fn insert_value(&mut self, old_value: usize, new_value: usize) {
        match self {
            WindowNode::Leaf(value) => {
                if *value == old_value {
                    *self = WindowNode::new_branch(WindowNode::new_leaf(old_value), WindowNode::new_leaf(new_value));
                }
            }
            WindowNode::Branch { left, right } => {
                left.insert_value(old_value, new_value);
                right.insert_value(old_value, new_value);
            }
        }
    }

    /// Return every leaf in the target leaf's sibling subtree.
    ///
    /// The sibling is not necessarily a single leaf: after repeatedly
    /// splitting one side of a layout, closing the other side must expand the
    /// complete remaining subtree into the released area.
    pub fn get_sibling_leaves(&self, value: usize) -> Option<Vec<usize>> {
        match self {
            WindowNode::Branch { left, right } => {
                if matches!(&**left, WindowNode::Leaf(left_value) if *left_value == value) {
                    let mut leaves = Vec::new();
                    right.collect_leaves(&mut leaves);
                    return Some(leaves);
                }
                if matches!(&**right, WindowNode::Leaf(right_value) if *right_value == value) {
                    let mut leaves = Vec::new();
                    left.collect_leaves(&mut leaves);
                    return Some(leaves);
                }

                left.get_sibling_leaves(value).or_else(|| right.get_sibling_leaves(value))
            }
            WindowNode::Leaf(_) => None,
        }
    }

    fn collect_leaves(&self, leaves: &mut Vec<usize>) {
        match self {
            WindowNode::Leaf(value) => leaves.push(*value),
            WindowNode::Branch { left, right } => {
                left.collect_leaves(leaves);
                right.collect_leaves(leaves);
            }
        }
    }

    /// Returns whether the leaf has been successfully removed
    pub fn remove_leaf(&mut self, value: usize) -> bool {
        if let WindowNode::Branch { left, right } = self {
            if matches!(&**left, WindowNode::Leaf(left_value) if *left_value == value) {
                *self = *right.clone();
                return true;
            }
            if matches!(&**right, WindowNode::Leaf(right_value) if *right_value == value) {
                *self = *left.clone();
                return true;
            }

            return left.remove_leaf(value) || right.remove_leaf(value);
        }
        false
    }

    pub fn swap_values(&mut self, value1: usize, value2: usize) {
        let mut node1 = None;
        let mut node2 = None;
        self.find_nodes_with_values(value1, value2, &mut node1, &mut node2);

        if let (Some(n1), Some(n2)) = (node1, node2) {
            if let (WindowNode::Leaf(ref mut v1), WindowNode::Leaf(ref mut v2)) = (&mut *n1, &mut *n2) {
                core::mem::swap(v1, v2);
            }
        }
    }

    fn find_nodes_with_values<'a>(&'a mut self, value1: usize, value2: usize, node1: &mut Option<&'a mut WindowNode>, node2: &mut Option<&'a mut WindowNode>) {
        match self {
            WindowNode::Leaf(v) => {
                if *v == value1 {
                    *node1 = Some(self);
                } else if *v == value2 {
                    *node2 = Some(self);
                }
            }
            WindowNode::Branch { left, right } => {
                left.find_nodes_with_values(value1, value2, node1, node2);
                right.find_nodes_with_values(value1, value2, node1, node2);
            }
        }
    }
}
