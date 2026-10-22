//! The category tree as a read model: built from every category (`CategoryRepo::list_all`;
//! a store has tens or hundreds), it answers parents, ancestors, descendants and depth in
//! memory. Commands check the tree's rules on it before writing, so storage needs no path
//! column a move could leave wrong.

use std::collections::HashMap;

use hypha_core::CategoryId;

use crate::{Category, CategoryError};

/// Every category, indexed by ID and by parent.
#[derive(Debug, Clone)]
pub struct CategoryTree {
    categories: HashMap<CategoryId, Category>,
    /// Each parent's children (`None`: the top level), ordered by position then ID.
    children: HashMap<Option<CategoryId>, Vec<CategoryId>>,
}

impl CategoryTree {
    pub fn new(categories: Vec<Category>) -> Self {
        let mut children: HashMap<Option<CategoryId>, Vec<CategoryId>> = HashMap::new();
        for c in &categories {
            children.entry(c.parent()).or_default().push(c.id());
        }
        let categories: HashMap<CategoryId, Category> =
            categories.into_iter().map(|c| (c.id(), c)).collect();
        for siblings in children.values_mut() {
            siblings.sort_by_key(|id| (categories[id].position(), id.as_uuid()));
        }
        Self {
            categories,
            children,
        }
    }

    pub fn get(&self, id: CategoryId) -> Option<&Category> {
        self.categories.get(&id)
    }

    pub fn contains(&self, id: CategoryId) -> bool {
        self.categories.contains_key(&id)
    }

    /// The children of `parent` (`None`: the top level), in order.
    pub fn children(&self, parent: Option<CategoryId>) -> Vec<&Category> {
        self.children
            .get(&parent)
            .map(|ids| ids.iter().map(|id| &self.categories[id]).collect())
            .unwrap_or_default()
    }

    /// `id`'s ancestors, nearest first (its parent, its parent's parent, …). A parent
    /// missing from the tree ends the walk, and so does a cycle stored data shouldn't
    /// have, so this always terminates.
    pub fn ancestors(&self, id: CategoryId) -> Vec<CategoryId> {
        let mut ancestors = Vec::new();
        let mut current = self.get(id).and_then(Category::parent);
        while let Some(parent) = current {
            if parent == id || ancestors.contains(&parent) || !self.contains(parent) {
                break;
            }
            ancestors.push(parent);
            current = self.get(parent).and_then(Category::parent);
        }
        ancestors
    }

    /// Every category below `id`, depth first in order (not `id` itself).
    pub fn descendants(&self, id: CategoryId) -> Vec<CategoryId> {
        let mut found = Vec::new();
        let mut stack: Vec<CategoryId> = self.child_ids(Some(id)).into_iter().rev().collect();
        while let Some(next) = stack.pop() {
            if next == id || found.contains(&next) {
                continue;
            }
            found.push(next);
            stack.extend(self.child_ids(Some(next)).into_iter().rev());
        }
        found
    }

    fn child_ids(&self, parent: Option<CategoryId>) -> Vec<CategoryId> {
        self.children.get(&parent).cloned().unwrap_or_default()
    }

    /// How deep `id` sits: 1 at the top level.
    pub fn depth(&self, id: CategoryId) -> usize {
        self.ancestors(id).len() + 1
    }

    /// How many levels `id`'s subtree spans: 1 for a category without children.
    /// Built from `descendants` and `depth`, which both stop at a stored cycle.
    pub fn height(&self, id: CategoryId) -> usize {
        let depth = self.depth(id);
        self.descendants(id)
            .iter()
            .map(|d| self.depth(*d).saturating_sub(depth) + 1)
            .max()
            .unwrap_or(1)
    }

    /// Whether the storefront shows `id`: it and every ancestor are visible.
    pub fn is_shown(&self, id: CategoryId) -> bool {
        self.get(id).is_some_and(Category::visible)
            && self
                .ancestors(id)
                .iter()
                .all(|a| self.get(*a).is_some_and(Category::visible))
    }

    /// Checks that `id` (with its subtree) may sit under `parent`: no cycle, and the
    /// deepest category stays within `max_depth`. `id` is `None` for a new category.
    pub fn check_placement(
        &self,
        id: Option<CategoryId>,
        parent: Option<CategoryId>,
        max_depth: u8,
    ) -> Result<(), CategoryError> {
        let Some(parent) = parent else {
            return self.check_depth(1, id, max_depth);
        };
        if let Some(id) = id
            && (parent == id || self.descendants(id).contains(&parent))
        {
            return Err(CategoryError::Cycle {
                category: id,
                parent,
            });
        }
        self.check_depth(self.depth(parent) + 1, id, max_depth)
    }

    fn check_depth(
        &self,
        depth: usize,
        id: Option<CategoryId>,
        max_depth: u8,
    ) -> Result<(), CategoryError> {
        let height = id.map_or(1, |id| self.height(id));
        if depth + height - 1 > usize::from(max_depth) {
            return Err(CategoryError::TooDeep { max: max_depth });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::Handle;
    use uuid::Uuid;

    use super::*;
    use crate::NewCategory;

    fn cid(n: u128) -> CategoryId {
        CategoryId::from_uuid(Uuid::from_u128(n))
    }

    fn cat(n: u128, parent: Option<u128>, position: u32, visible: bool) -> Category {
        Category::new(NewCategory {
            id: cid(n),
            name: format!("C{n}"),
            handle: Handle::new(&format!("c{n}")).unwrap(),
            description: None,
            parent: parent.map(cid),
            position,
            visible,
        })
        .unwrap()
    }

    /// 1 Clothing › 2 Shirts › 4 Linen; 1 › 3 Trousers; 5 Sale (hidden).
    fn tree() -> CategoryTree {
        CategoryTree::new(vec![
            cat(4, Some(2), 0, true),
            cat(3, Some(1), 1, true),
            cat(2, Some(1), 0, true),
            cat(1, None, 0, true),
            cat(5, None, 1, false),
        ])
    }

    #[test]
    fn children_come_in_position_order() {
        let t = tree();
        let ids = |cs: Vec<&Category>| cs.iter().map(|c| c.id()).collect::<Vec<_>>();
        assert_eq!(ids(t.children(None)), [cid(1), cid(5)]);
        assert_eq!(ids(t.children(Some(cid(1)))), [cid(2), cid(3)]);
    }

    #[test]
    fn ancestors_are_nearest_first_and_descendants_depth_first() {
        let t = tree();
        assert_eq!(t.ancestors(cid(4)), [cid(2), cid(1)]);
        assert_eq!(t.descendants(cid(1)), [cid(2), cid(4), cid(3)]);
        assert_eq!((t.depth(cid(4)), t.height(cid(1))), (3, 3));
    }

    #[test]
    fn a_hidden_ancestor_hides_its_subtree() {
        let t = CategoryTree::new(vec![cat(1, None, 0, false), cat(2, Some(1), 0, true)]);
        assert!(!t.is_shown(cid(2)));
        assert!(tree().is_shown(cid(4)));
    }

    #[test]
    fn moving_under_itself_or_a_descendant_is_a_cycle() {
        let t = tree();
        for parent in [1, 2, 4] {
            assert_eq!(
                t.check_placement(Some(cid(1)), Some(cid(parent)), 5),
                Err(CategoryError::Cycle {
                    category: cid(1),
                    parent: cid(parent)
                })
            );
        }
        assert_eq!(t.check_placement(Some(cid(4)), None, 5), Ok(()));
    }

    #[test]
    fn the_depth_limit_counts_the_moved_subtree() {
        let t = tree();
        // Linen is at depth 3: a new child is at 4.
        assert_eq!(t.check_placement(None, Some(cid(4)), 4), Ok(()));
        assert_eq!(
            t.check_placement(None, Some(cid(4)), 3),
            Err(CategoryError::TooDeep { max: 3 })
        );
        // Clothing's subtree spans 3 levels: under Sale it would reach 4.
        assert_eq!(t.check_placement(Some(cid(1)), Some(cid(5)), 4), Ok(()));
        assert_eq!(
            t.check_placement(Some(cid(1)), Some(cid(5)), 3),
            Err(CategoryError::TooDeep { max: 3 })
        );
    }

    #[test]
    fn a_stored_cycle_still_terminates() {
        let t = CategoryTree::new(vec![cat(1, Some(2), 0, true), cat(2, Some(1), 0, true)]);
        assert_eq!(t.ancestors(cid(1)), [cid(2)]);
        assert_eq!(t.descendants(cid(1)), [cid(2)]);
        assert_eq!(t.height(cid(1)), 1);
    }
}
