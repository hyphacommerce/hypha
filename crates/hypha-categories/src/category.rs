use hypha_core::{
    AssetId, CategoryId, Description, Handle, PendingEvent, Recorded, Stamped, Stamps, Version,
    Versioned, required_text,
};

use crate::{CategoryError, events::Changes};

/// A category of products: a name and handle, an optional description and image, its place
/// in the tree (a parent and a position among its siblings), and whether the storefront
/// shows it. Which products are in it is its membership (`CategoryRepo::add_products`),
/// kept apart since it grows with the catalog.
///
/// The tree's rules (no cycles, the depth limit, dense sibling positions) span several
/// categories, so they're checked by the commands on a `CategoryTree`; `place` only
/// records where the category now is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    id: CategoryId,
    version: Version,
    stamps: Option<Stamps>,
    name: String,
    handle: Handle,
    description: Option<Description>,
    image: Option<AssetId>,
    parent: Option<CategoryId>,
    position: u32,
    visible: bool,
    changes: Changes,
}

/// What a new category is made of, checked by `Category::new`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCategory {
    pub id: CategoryId,
    pub name: String,
    pub handle: Handle,
    pub description: Option<Description>,
    pub parent: Option<CategoryId>,
    pub position: u32,
    /// Stated, never defaulted.
    pub visible: bool,
}

impl Category {
    pub fn new(new: NewCategory) -> Result<Self, CategoryError> {
        Ok(Self {
            id: new.id,
            version: Version::NEW,
            stamps: None,
            name: required_text("name", &new.name)?,
            handle: new.handle,
            description: new.description,
            image: None,
            parent: new.parent,
            position: new.position,
            visible: new.visible,
            changes: Changes::created(),
        })
    }

    /// The shared text rule; the same name records nothing.
    pub fn rename(&mut self, name: &str) -> Result<(), CategoryError> {
        let name = required_text("name", name)?;
        if name != self.name {
            self.name = name;
            self.changes.field("name");
        }
        Ok(())
    }

    pub fn set_handle(&mut self, handle: Handle) {
        if handle != self.handle {
            self.handle = handle;
            self.changes.field("handle");
        }
    }

    /// Sanitised HTML (`Description`); `None` clears it.
    pub fn set_description(&mut self, description: Option<Description>) {
        if description != self.description {
            self.description = description;
            self.changes.field("description");
        }
    }

    /// The image from the media library; `hypha-merchandising` sets it, together with the
    /// asset's usage.
    pub fn set_image(&mut self, image: Option<AssetId>) {
        if image != self.image {
            self.image = image;
            self.changes.field("image");
        }
    }

    /// `false` hides it and everything below it from the storefront.
    pub fn set_visible(&mut self, visible: bool) {
        if visible != self.visible {
            self.visible = visible;
            self.changes.field("visible");
        }
    }

    /// Where it sits now: under `parent` (`None`: at the top), at `position` among its
    /// siblings. The commands check the tree first and renumber the siblings.
    pub(crate) fn place(&mut self, parent: Option<CategoryId>, position: u32) {
        if parent != self.parent {
            self.parent = parent;
            self.changes.field("parent");
        }
        if position != self.position {
            self.position = position;
            self.changes.field("position");
        }
    }

    pub fn id(&self) -> CategoryId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn handle(&self) -> &Handle {
        &self.handle
    }

    pub fn description(&self) -> Option<&Description> {
        self.description.as_ref()
    }

    pub fn image(&self) -> Option<AssetId> {
        self.image
    }

    pub fn parent(&self) -> Option<CategoryId> {
        self.parent
    }

    pub fn position(&self) -> u32 {
        self.position
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn snapshot(&self) -> CategorySnapshot {
        CategorySnapshot {
            id: self.id,
            version: self.version,
            stamps: self.stamps,
            name: self.name.clone(),
            handle: self.handle.as_str().to_string(),
            description: self.description.as_ref().map(|d| d.as_html().to_string()),
            image: self.image,
            parent: self.parent,
            position: self.position,
            visible: self.visible,
        }
    }

    /// Rebuilds a category from storage, with the rules `new` checks. The description
    /// is taken as stored (`Description::stored`): it was sanitised on the way in.
    pub fn restore(s: CategorySnapshot) -> Result<Category, CategoryError> {
        let mut category = Category::new(NewCategory {
            id: s.id,
            name: s.name,
            handle: Handle::new(&s.handle)?,
            description: s
                .description
                .map(Description::stored)
                .transpose()?
                .flatten(),
            parent: s.parent,
            position: s.position,
            visible: s.visible,
        })?;
        category.version = s.version;
        category.stamps = s.stamps;
        category.image = s.image;
        category.changes = Changes::default();
        Ok(category)
    }
}

impl Recorded for Category {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.changes.take(self.id)
    }
}

impl Versioned for Category {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Stamped for Category {
    fn stamps(&self) -> Option<Stamps> {
        self.stamps
    }

    fn set_stamps(&mut self, stamps: Stamps) {
        self.stamps = Some(stamps);
    }
}

/// Plain-data form of a `Category` for storage adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategorySnapshot {
    pub id: CategoryId,
    pub version: Version,
    pub stamps: Option<Stamps>,
    pub name: String,
    pub handle: String,
    /// Sanitised HTML, as `Description` stored it.
    pub description: Option<String>,
    pub image: Option<AssetId>,
    pub parent: Option<CategoryId>,
    pub position: u32,
    pub visible: bool,
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn cid(n: u128) -> CategoryId {
        CategoryId::from_uuid(Uuid::from_u128(n))
    }

    fn shirts() -> Category {
        Category::new(NewCategory {
            id: cid(1),
            name: " Shirts ".to_string(),
            handle: Handle::new("shirts").unwrap(),
            description: Description::new("<p>Linen</p>").unwrap(),
            parent: Some(cid(9)),
            position: 2,
            visible: false,
        })
        .unwrap()
    }

    fn fields(c: &mut Category) -> Vec<String> {
        c.take_events()
            .iter()
            .flat_map(|e| e.data["fields"].as_array().cloned().unwrap_or_default())
            .map(|f| f.as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn new_trims_the_name_and_records_its_creation() {
        let mut c = shirts();
        assert_eq!(c.name(), "Shirts");
        let events = c.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data["created"], serde_json::json!(true));
    }

    #[test]
    fn rejects_a_blank_name() {
        let result = Category::new(NewCategory {
            name: "  ".to_string(),
            ..shirts().as_new()
        });
        assert_eq!(
            result,
            Err(CategoryError::Text(hypha_core::TextError::Missing("name")))
        );
    }

    #[test]
    fn changes_are_coalesced_and_unchanged_values_record_nothing() {
        let mut c = shirts();
        c.take_events();
        c.rename("Shirts").unwrap();
        c.set_visible(false);
        assert!(c.take_events().is_empty());
        c.rename("Tops").unwrap();
        c.set_visible(true);
        c.place(None, 0);
        assert_eq!(fields(&mut c), ["name", "parent", "position", "visible"]);
    }

    #[test]
    fn restore_round_trips() {
        let mut c = shirts();
        c.set_image(Some(AssetId::from_uuid(Uuid::from_u128(5))));
        c.set_version(Version::new(3));
        c.take_events();
        assert_eq!(Category::restore(c.snapshot()), Ok(c));
    }

    impl Category {
        fn as_new(&self) -> NewCategory {
            NewCategory {
                id: self.id,
                name: self.name.clone(),
                handle: self.handle.clone(),
                description: self.description.clone(),
                parent: self.parent,
                position: self.position,
                visible: self.visible,
            }
        }
    }
}
