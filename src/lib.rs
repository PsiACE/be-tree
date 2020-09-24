use std::mem;
use std::ptr;

const MAX_VALUES_PER_LEAF: usize = 4;

/* A pivot is a key and a node of the subtree of values >= that key. */
struct Pivot<K, V> {
    min_key: K,
    child: Box<Node<K, V>>,
}

struct LeafNode<K, V> {
    elements: [(K, V); MAX_VALUES_PER_LEAF],
    // must be <= MAX_VALUES_PER_LEAF
    len: usize,
}

impl<K, V> LeafNode<K, V>
where
    K: Copy,
    V: Clone,
{
    fn empty() -> Self {
        unsafe { Self { elements: mem::MaybeUninit::uninit().assume_init(), len: 0 } }
    }

    fn from(items: &[(K, V)]) -> Self {
        debug_assert!(items.len() <= MAX_VALUES_PER_LEAF);
        let mut result = Self::empty();
        result.elements.clone_from_slice(items);
        result
    }

    fn valid_elements_mut(&mut self) -> &mut [(K, V)] {
        &mut self.elements[0..self.len]
    }

    fn valid_elements(&self) -> &[(K, V)] {
        &self.elements[0..self.len]
    }
}

struct BranchNode<K, V> {
    pivots: [Pivot<K, V>; MAX_VALUES_PER_LEAF],
    // must be <= MAX_VALUES_PER_LEAF and > 1
    len: usize,
}

impl<K, V> BranchNode<K, V>
where
    K: Copy,
{
    fn from(left: Pivot<K, V>, right: Pivot<K, V>) -> Self {
        unsafe {
            let mut result = Self { pivots: mem::MaybeUninit::uninit().assume_init(), len: 2 };
            result.pivots[0] = left;
            result.pivots[1] = right;
            result
        }
    }
    fn valid_pivots_mut(&mut self) -> &mut [Pivot<K, V>] {
        &mut self.pivots[0..self.len]
    }

    fn valid_pivots(&self) -> &[Pivot<K, V>] {
        &self.pivots[0..self.len]
    }
}

enum Node<K, V> {
    Branch(BranchNode<K, V>),
    Leaf(LeafNode<K, V>),
}

impl<K, V> Node<K, V>
where
    K: Copy + Ord,
    V: Clone,
{
    fn min_key(&self) -> K {
        match *self {
            Node::Branch(ref branch) => {
                debug_assert!(branch.len > 1);
                branch.pivots[0].min_key
            }
            Node::Leaf(ref leaf) => {
                debug_assert_ne!(leaf.len, 0);
                leaf.elements[0].0
            }
        }
    }

    fn insert(&mut self, key: K, value: V) {
        let replace_node: Option<Self> = match *self {
            Node::Branch(ref mut branch) => {
                // Find a child node whose keys are not before the target key
                match branch.valid_pivots().iter().position(|ref p| key <= p.min_key) {
                    Some(i) => {
                        // If there is one, insert into it and update the pivot key
                        let pivot = &mut branch.pivots[i];
                        pivot.min_key = key;
                        pivot.child.insert(key, value)
                    }
                    // o/w, insert a new leaf at the end
                    None => {
                        branch.pivots[branch.len] =
                            Pivot { min_key: key, child: Box::new(Node::Leaf(LeafNode::empty())) };
                        branch.len += 1
                        // XXX consider splitting branch
                    }
                };
                None
            }
            Node::Leaf(ref mut leaf) => {
                let index = leaf.valid_elements_mut().binary_search_by_key(&key, |&(k, _)| k);
                match index {
                    Err(i) => {
                        // key is absent, true insert
                        if leaf.len < MAX_VALUES_PER_LEAF {
                            // there's space left, just insert
                            unsafe { slice_insert(leaf.valid_elements_mut(), i, (key, value)) }
                            leaf.len += 1;
                            None
                        } else {
                            // must split the node: create the new node here
                            let new_branch = {
                                let (left, right) =
                                    leaf.valid_elements_mut().split_at(MAX_VALUES_PER_LEAF / 2);
                                let left_leaf = Box::new(Node::Leaf(LeafNode::from(left)));
                                let right_leaf = Box::new(Node::Leaf(LeafNode::from(right)));
                                Node::Branch(BranchNode::from(
                                    Pivot { min_key: left_leaf.min_key(), child: left_leaf },
                                    Pivot { min_key: right_leaf.min_key(), child: right_leaf },
                                ))
                            };
                            Some(new_branch)
                        }
                    }
                    // key is present, replace
                    Ok(i) => {
                        leaf.elements[i] = (key, value);
                        None
                    }
                }
            }
        };
        if let Some(new_branch) = replace_node {
            *self = new_branch
        }
    }

    fn delete(&mut self, key: K) {
        match *self {
            Node::Branch(ref mut branch) => {
                // Find a child node whose keys are not before the target key
                if let Some(ref mut pivot) =
                    branch.valid_pivots_mut().iter_mut().find(|ref p| key <= p.min_key)
                {
                    // If there is one, delete from it and update the pivot key
                    pivot.child.delete(key);
                    pivot.min_key = pivot.child.min_key()
                }
            }
            Node::Leaf(ref mut leaf) if leaf.len > 0 => {
                let index = leaf.valid_elements_mut().binary_search_by_key(&key, |&(k, _)| k);
                match index {
                    Err(_) => (),
                    Ok(i) => unsafe {
                        slice_remove(leaf.valid_elements_mut(), i);
                        leaf.len -= 1;
                    },
                }
            }
            _ => (),
        }
    }

    fn get(&self, key: K) -> Option<&V> {
        match *self {
            Node::Branch(ref branch) => {
                // Find a child node whose keys are not before the target key
                match branch.valid_pivots().iter().find(|ref p| key <= p.min_key) {
                    Some(ref pivot) => {
                        // If there is one, query it
                        pivot.child.get(key)
                    }
                    // o/w, the key doesn't exist
                    None => None,
                }
            }
            Node::Leaf(ref leaf) if leaf.len > 0 => {
                let index = leaf.valid_elements().binary_search_by_key(&key, |&(k, _)| k);
                match index {
                    Err(_) => None,
                    Ok(i) => Some(&leaf.elements[i].1),
                }
            }
            _ => None,
        }
    }
}

unsafe fn slice_insert<T>(slice: &mut [T], idx: usize, val: T) {
    ptr::copy(
        slice.as_mut_ptr().add(idx),
        slice.as_mut_ptr().offset(idx as isize + 1),
        slice.len() - idx,
    );
    ptr::write(slice.get_unchecked_mut(idx), val);
}

unsafe fn slice_remove<T>(slice: &mut [T], idx: usize) -> T {
    let ret = ptr::read(slice.get_unchecked(idx));
    ptr::copy(
        slice.as_ptr().offset(idx as isize + 1),
        slice.as_mut_ptr().add(idx),
        slice.len() - idx - 1,
    );
    ret
}

/// A map based on a B𝛆-tree
pub struct BeTree<K, V> {
    root: Node<K, V>,
}

impl<K, V> BeTree<K, V>
where
    K: Copy + Ord,
    V: Clone,
{
    /// Create an empty B𝛆-tree.
    pub fn new() -> Self {
        BeTree { root: Node::Leaf(LeafNode::empty()) }
    }

    /// Clear the tree, removing all entries.
    pub fn clear(&mut self) {
        match self.root {
            Node::Leaf(ref mut leaf) => leaf.len = 0,
            _ => self.root = Node::Leaf(LeafNode::empty()),
        }
    }

    /// Insert a key-value pair into the tree.
    ///
    /// If the key is already present in the tree, the value is replaced. The key is not updated, though; this matters for
    /// types that can be `==` without being identical.
    pub fn insert(&mut self, key: K, value: V) {
        self.root.insert(key, value)
    }

    /// Remove a key (and its value) from the tree.
    ///
    /// If the key is not present, silently does nothing.
    pub fn delete(&mut self, key: K) {
        self.root.delete(key)
    }

    /// Retrieve a reference to the value corresponding to the key.
    pub fn get(&self, key: K) -> Option<&V> {
        self.root.get(key)
    }
}

impl<K, V> Default for BeTree<K, V>
where
    K: Copy + Ord,
    V: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{BeTree, MAX_VALUES_PER_LEAF};

    #[test]
    fn can_construct() {
        BeTree::<i32, char>::new();
    }

    #[test]
    fn can_insert_single() {
        let mut b = BeTree::new();
        b.insert(0, 'x');
        let result = b.get(0);
        assert_eq!(Some(&'x'), result);
    }

    #[test]
    fn can_insert_two() {
        let mut b = BeTree::new();
        b.insert(0, 'x');
        b.insert(-1, 'y');
        assert_eq!(Some(&'x'), b.get(0));
        assert_eq!(Some(&'y'), b.get(-1));
    }

    #[test]
    fn can_split() {
        let mut b = BeTree::new();
        // insert MAX_VALUES_PER_LEAF + 1 items
        for i in 0..MAX_VALUES_PER_LEAF {
            b.insert(i, i);
        }
        // are they all there?
        for i in 0..MAX_VALUES_PER_LEAF {
            assert_eq!(Some(&i), b.get(i));
        }
    }

    #[test]
    fn can_clear() {
        let mut b = BeTree::new();
        b.insert(0, 'x');
        b.insert(-1, 'y');
        b.clear();
        assert_eq!(None, b.get(0));
    }

    #[test]
    fn insert_replaces_existing() {
        let mut b = BeTree::new();
        b.insert(0, 'x');
        b.insert(0, 'y');
        assert_eq!(Some(&'y'), b.get(0));
    }

    #[test]
    fn can_delete_existing() {
        let mut b = BeTree::new();
        b.insert(0, 'x');
        b.delete(0);
        assert_eq!(b.get(0), None)
    }

    #[test]
    fn can_delete_only_existing() {
        let mut b = BeTree::new();
        b.insert(0, 'x');
        b.insert(2, 'y');
        b.delete(0);
        assert_eq!(b.get(0), None);
        assert_eq!(Some(&'y'), b.get(2));
    }

    #[test]
    fn can_delete_nothing() {
        let mut b = BeTree::<i32, char>::new();
        b.delete(0);
    }
}
