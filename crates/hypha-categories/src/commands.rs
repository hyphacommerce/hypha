//! Managing the category tree and what's in it (design note 0022). Categories organise
//! products, so they use the products scopes (CA8).

use std::collections::{BTreeSet, HashMap};

use hypha_core::{
    AuthError, CategoryId, Ctx, Description, ErrorCode, Handle, HasBatch, IdGenerator, Page,
    PageRequest, PendingEvent, ProductId, RepoError, Scope, StageEvents, Subject,
};
use thiserror::Error;

use crate::{
    Category, CategoryError, CategoryProductsAdded, CategoryProductsRemoved, CategoryRepo,
    CategorySettings, CategoryTree, HasCategories, NewCategory, StageCategories,
    events::{CATEGORY, CategoryDeleted},
    repo::keys,
};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Category(#[from] CategoryError),
    #[error("category {0} not found")]
    CategoryNotFound(CategoryId),
    /// No category (or none the storefront shows) has this handle.
    #[error("no category has the handle {0:?}")]
    HandleNotFound(String),
    #[error("parent category {0} not found")]
    ParentNotFound(CategoryId),
    #[error("a category already has the handle {0:?}")]
    HandleTaken(String),
    /// Move or delete its children first (CA7).
    #[error("category {0} has subcategories")]
    HasChildren(CategoryId),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Category(e) => e.code(),
            CommandError::CategoryNotFound(_) | CommandError::HandleNotFound(_) => {
                "category_not_found"
            }
            CommandError::ParentNotFound(_) => "category_parent_not_found",
            CommandError::HandleTaken(_) => "category_handle_taken",
            CommandError::HasChildren(_) => "category_has_children",
        }
    }
}

/// Every port the category writes use, so hosts and tests name one bound.
pub trait CategoryPorts: HasCategories + HasBatch<Batch: StageCategories + StageEvents> {}

impl<T> CategoryPorts for T where T: HasCategories + HasBatch<Batch: StageCategories + StageEvents> {}

fn subject(category: CategoryId) -> Subject {
    Subject::new(CATEGORY, category.as_uuid())
}

async fn tree<S: HasCategories>(ports: &S) -> Result<CategoryTree, RepoError> {
    Ok(CategoryTree::new(ports.categories().list_all().await?))
}

/// A `Duplicate` on the handle from the write is the same error as the check before
/// it: a race looks like a normal duplicate.
fn handle_taken(e: RepoError, handle: &Handle) -> CommandError {
    match e {
        RepoError::Duplicate(keys::CATEGORY_HANDLE) => {
            CommandError::HandleTaken(handle.as_str().to_string())
        }
        e => e.into(),
    }
}

async fn check_handle_free<S: HasCategories>(
    ports: &S,
    handle: &Handle,
    own: Option<CategoryId>,
) -> Result<(), CommandError> {
    match ports.categories().get_by_handle(handle.as_str()).await? {
        Some(other) if Some(other.id()) != own => {
            Err(CommandError::HandleTaken(handle.as_str().to_string()))
        }
        _ => Ok(()),
    }
}

fn description(html: Option<&str>) -> Result<Option<Description>, CommandError> {
    Ok(html
        .map(Description::new)
        .transpose()
        .map_err(CategoryError::from)?
        .flatten())
}

/// The siblings of `order`'s parent whose position isn't their index, placed there:
/// what to stage so positions stay 0 to n − 1. `skip` is staged by the caller.
fn renumbered(
    tree: &CategoryTree,
    parent: Option<CategoryId>,
    order: &[CategoryId],
    skip: CategoryId,
) -> Vec<Category> {
    order
        .iter()
        .enumerate()
        .filter(|(_, id)| **id != skip)
        .filter_map(|(index, id)| {
            let index = u32::try_from(index).unwrap_or(u32::MAX);
            let sibling = tree.get(*id)?;
            (sibling.position() != index || sibling.parent() != parent).then(|| {
                let mut moved = sibling.clone();
                moved.place(parent, index);
                moved
            })
        })
        .collect()
}

/// `id` inserted among `parent`'s other children at `position` (clamped to the end).
fn order_with(
    tree: &CategoryTree,
    parent: Option<CategoryId>,
    id: CategoryId,
    position: Option<u32>,
) -> Vec<CategoryId> {
    let mut order: Vec<CategoryId> = tree
        .children(parent)
        .iter()
        .map(|c| c.id())
        .filter(|c| *c != id)
        .collect();
    let at = position.map_or(order.len(), |p| (p as usize).min(order.len()));
    order.insert(at, id);
    order
}

/// A new category, unvalidated. `visible` is required (CA10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateCategory {
    pub name: String,
    pub handle: String,
    /// HTML, sanitised (`Description`).
    pub description: Option<String>,
    pub parent: Option<CategoryId>,
    /// Among its siblings; the end when `None` or past it.
    pub position: Option<u32>,
    pub visible: bool,
}

pub async fn create_category<S: CategoryPorts>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    settings: &CategorySettings,
    input: CreateCategory,
) -> Result<Category, CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    let handle = Handle::new(&input.handle).map_err(CategoryError::from)?;
    let description = description(input.description.as_deref())?;
    let tree = tree(ports).await?;
    if let Some(parent) = input.parent
        && !tree.contains(parent)
    {
        return Err(CommandError::ParentNotFound(parent));
    }
    tree.check_placement(None, input.parent, settings.max_depth)?;
    check_handle_free(ports, &handle, None).await?;

    let id = CategoryId::from_uuid(ids.new_uuid());
    let order = order_with(&tree, input.parent, id, input.position);
    let position = order.iter().position(|c| *c == id).unwrap_or(0);
    let mut category = Category::new(NewCategory {
        id,
        name: input.name,
        handle: handle.clone(),
        description,
        parent: input.parent,
        position: u32::try_from(position).unwrap_or(u32::MAX),
        visible: input.visible,
    })?;
    let mut batch = ports.batch();
    batch.insert_category(&mut category);
    for mut sibling in renumbered(&tree, input.parent, &order, id) {
        batch.update_category(&mut sibling);
    }
    ports
        .commit(batch)
        .await
        .map_err(|e| handle_taken(e, &handle))?;
    Ok(category)
}

/// Changes only what's `Some`, like a JSON Merge Patch: `description: Some(None)`
/// clears it, `parent: Some(None)` moves it to the top level.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateCategory {
    pub name: Option<String>,
    pub handle: Option<String>,
    pub description: Option<Option<String>>,
    pub visible: Option<bool>,
    pub parent: Option<Option<CategoryId>>,
    /// Among its (new) siblings; moving without one puts it at the end.
    pub position: Option<u32>,
}

/// Renames, describes, shows or hides, or moves a category. A move is checked on the
/// tree (no cycles, the depth limit counting its subtree) and renumbers the old and
/// new siblings in the same batch.
pub async fn update_category<S: CategoryPorts>(
    ctx: &Ctx,
    ports: &S,
    settings: &CategorySettings,
    id: CategoryId,
    input: UpdateCategory,
) -> Result<Category, CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    let handle = input
        .handle
        .as_deref()
        .map(Handle::new)
        .transpose()
        .map_err(CategoryError::from)?;
    let new_description = input
        .description
        .as_ref()
        .map(|d| description(d.as_deref()))
        .transpose()?;
    let tree = tree(ports).await?;
    let mut category = tree
        .get(id)
        .cloned()
        .ok_or(CommandError::CategoryNotFound(id))?;

    if let Some(name) = &input.name {
        category.rename(name)?;
    }
    if let Some(handle) = &handle {
        check_handle_free(ports, handle, Some(id)).await?;
        category.set_handle(handle.clone());
    }
    if let Some(description) = new_description {
        category.set_description(description);
    }
    if let Some(visible) = input.visible {
        category.set_visible(visible);
    }

    let old_parent = category.parent();
    let new_parent = input.parent.unwrap_or(old_parent);
    let mut siblings = Vec::new();
    if input.parent.is_some() || input.position.is_some() {
        if new_parent != old_parent {
            if let Some(parent) = new_parent
                && !tree.contains(parent)
            {
                return Err(CommandError::ParentNotFound(parent));
            }
            tree.check_placement(Some(id), new_parent, settings.max_depth)?;
        }
        let position = match (input.position, new_parent == old_parent) {
            (Some(position), _) => Some(position),
            (None, true) => Some(category.position()),
            (None, false) => None,
        };
        let order = order_with(&tree, new_parent, id, position);
        let index = order.iter().position(|c| *c == id).unwrap_or(0);
        category.place(new_parent, u32::try_from(index).unwrap_or(u32::MAX));
        siblings = renumbered(&tree, new_parent, &order, id);
        if new_parent != old_parent {
            let rest: Vec<CategoryId> = tree
                .children(old_parent)
                .iter()
                .map(|c| c.id())
                .filter(|c| *c != id)
                .collect();
            siblings.extend(renumbered(&tree, old_parent, &rest, id));
        }
    }

    // Nothing changed writes nothing: no new version for a concurrent edit to lose to.
    // (`Category`'s equality compares fields, never pending events.)
    if Some(&category) == tree.get(id) && siblings.is_empty() {
        return Ok(category);
    }
    let mut batch = ports.batch();
    batch.update_category(&mut category);
    for sibling in &mut siblings {
        batch.update_category(sibling);
    }
    ports
        .commit(batch)
        .await
        .map_err(|e| handle_taken(e, category.handle()))?;
    Ok(category)
}

/// Deletes a category without subcategories (`HasChildren` otherwise), with its
/// membership; its products stay. Its siblings close the gap.
pub async fn delete_category<S: CategoryPorts>(
    ctx: &Ctx,
    ports: &S,
    id: CategoryId,
) -> Result<(), CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    let tree = tree(ports).await?;
    let category = tree.get(id).ok_or(CommandError::CategoryNotFound(id))?;
    if !tree.children(Some(id)).is_empty() {
        return Err(CommandError::HasChildren(id));
    }
    let products = ports.categories().products_in(&[id]).await?;
    let parent = category.parent();
    let rest: Vec<CategoryId> = tree
        .children(parent)
        .iter()
        .map(|c| c.id())
        .filter(|c| *c != id)
        .collect();
    let mut batch = ports.batch();
    batch.delete_category(id);
    for mut sibling in renumbered(&tree, parent, &rest, id) {
        batch.update_category(&mut sibling);
    }
    let event = CategoryDeleted {
        category: id,
        products,
    };
    batch.record_events(vec![PendingEvent::new(subject(id), &event)]);
    Ok(ports.commit(batch).await?)
}

/// A category with its place in the tree: what its page and a breadcrumb need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryView {
    pub category: Category,
    /// Nearest first.
    pub ancestors: Vec<Category>,
    /// In order.
    pub children: Vec<Category>,
}

impl CategoryView {
    fn of(tree: &CategoryTree, category: &Category, shown_only: bool) -> Self {
        let ancestors = tree
            .ancestors(category.id())
            .iter()
            .filter_map(|a| tree.get(*a).cloned())
            .collect();
        let children = tree
            .children(Some(category.id()))
            .into_iter()
            .filter(|c| !shown_only || c.visible())
            .cloned()
            .collect();
        Self {
            category: category.clone(),
            ancestors,
            children,
        }
    }
}

/// The whole tree, every category, for the admin.
pub async fn list_categories<S: HasCategories>(
    ctx: &Ctx,
    ports: &S,
) -> Result<CategoryTree, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    Ok(tree(ports).await?)
}

pub async fn get_category<S: HasCategories>(
    ctx: &Ctx,
    ports: &S,
    id: CategoryId,
) -> Result<CategoryView, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    let tree = tree(ports).await?;
    let category = tree.get(id).ok_or(CommandError::CategoryNotFound(id))?;
    Ok(CategoryView::of(&tree, category, false))
}

/// The tree a storefront shows: a hidden category is left out with everything under
/// it (CA4).
pub async fn shown_categories<S: HasCategories>(
    ctx: &Ctx,
    ports: &S,
) -> Result<CategoryTree, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    let tree = tree(ports).await?;
    let shown = tree
        .children(None)
        .into_iter()
        .flat_map(|top| std::iter::once(top.id()).chain(tree.descendants(top.id())))
        .filter(|id| tree.is_shown(*id))
        .filter_map(|id| tree.get(id).cloned())
        .collect();
    Ok(CategoryTree::new(shown))
}

/// A category the storefront shows, by handle, with its ancestors and shown children;
/// a hidden one (or one under a hidden one) reads as missing.
pub async fn get_shown_category_by_handle<S: HasCategories>(
    ctx: &Ctx,
    ports: &S,
    handle: &str,
) -> Result<CategoryView, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    let not_found = || CommandError::HandleNotFound(handle.to_string());
    let found = ports
        .categories()
        .get_by_handle(handle)
        .await?
        .ok_or_else(not_found)?;
    let tree = tree(ports).await?;
    if !tree.is_shown(found.id()) {
        return Err(not_found());
    }
    let category = tree.get(found.id()).ok_or_else(not_found)?;
    Ok(CategoryView::of(&tree, category, true))
}

/// Whether the storefront shows `id` (search's `category` filter, CA4).
pub async fn is_shown<S: HasCategories>(
    ctx: &Ctx,
    ports: &S,
    id: CategoryId,
) -> Result<bool, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    Ok(tree(ports).await?.is_shown(id))
}

/// Each once, in order.
fn distinct(products: Vec<ProductId>) -> Vec<ProductId> {
    let mut seen = BTreeSet::new();
    products.into_iter().filter(|p| seen.insert(*p)).collect()
}

async fn load<S: HasCategories>(ports: &S, id: CategoryId) -> Result<Category, CommandError> {
    ports
        .categories()
        .get(id)
        .await?
        .ok_or(CommandError::CategoryNotFound(id))
}

/// Which of `products` are directly in `category`.
async fn members<S: HasCategories>(
    ports: &S,
    category: CategoryId,
    products: &[ProductId],
) -> Result<BTreeSet<ProductId>, RepoError> {
    Ok(ports
        .categories()
        .categories_of(products)
        .await?
        .into_iter()
        .filter(|(_, categories)| categories.contains(&category))
        .map(|(product, _)| product)
        .collect())
}

/// Adds products to the category. Product IDs aren't checked (a leaf crate can't see
/// products): an unknown one is stored and never matches. Ones already in it change
/// nothing and aren't in the event.
pub async fn add_category_products<S: CategoryPorts>(
    ctx: &Ctx,
    ports: &S,
    category: CategoryId,
    products: Vec<ProductId>,
) -> Result<(), CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    load(ports, category).await?;
    let products = distinct(products);
    let already = members(ports, category, &products).await?;
    let new: Vec<ProductId> = products
        .into_iter()
        .filter(|p| !already.contains(p))
        .collect();
    if new.is_empty() {
        return Ok(());
    }
    let mut batch = ports.batch();
    batch.add_category_products(category, &new);
    let event = CategoryProductsAdded {
        category,
        products: new,
    };
    batch.record_events(vec![PendingEvent::new(subject(category), &event)]);
    Ok(ports.commit(batch).await?)
}

/// Removes products from the category; ones not in it change nothing.
pub async fn remove_category_products<S: CategoryPorts>(
    ctx: &Ctx,
    ports: &S,
    category: CategoryId,
    products: Vec<ProductId>,
) -> Result<(), CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    load(ports, category).await?;
    let products = distinct(products);
    let present = members(ports, category, &products).await?;
    let gone: Vec<ProductId> = products
        .into_iter()
        .filter(|p| present.contains(p))
        .collect();
    if gone.is_empty() {
        return Ok(());
    }
    let mut batch = ports.batch();
    batch.remove_category_products(category, &gone);
    let event = CategoryProductsRemoved {
        category,
        products: gone,
    };
    batch.record_events(vec![PendingEvent::new(subject(category), &event)]);
    Ok(ports.commit(batch).await?)
}

/// Makes `categories` the product's categories (the product page's card): adds it to
/// the new ones and removes it from the others, in one batch with an event per
/// category. Every category must exist (`CategoryNotFound`, from the body).
pub async fn set_product_categories<S: CategoryPorts>(
    ctx: &Ctx,
    ports: &S,
    product: ProductId,
    categories: Vec<CategoryId>,
) -> Result<Vec<CategoryId>, CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    let tree = tree(ports).await?;
    let wanted: BTreeSet<CategoryId> = categories.into_iter().collect();
    if let Some(unknown) = wanted.iter().find(|c| !tree.contains(**c)) {
        return Err(CommandError::CategoryNotFound(*unknown));
    }
    let current: BTreeSet<CategoryId> = ports
        .categories()
        .categories_of(&[product])
        .await?
        .remove(&product)
        .unwrap_or_default()
        .into_iter()
        .collect();
    if current == wanted {
        return Ok(wanted.into_iter().collect());
    }
    let mut batch = ports.batch();
    let mut events = Vec::new();
    for category in wanted.difference(&current) {
        batch.add_category_products(*category, &[product]);
        let event = CategoryProductsAdded {
            category: *category,
            products: vec![product],
        };
        events.push(PendingEvent::new(subject(*category), &event));
    }
    for category in current.difference(&wanted) {
        batch.remove_category_products(*category, &[product]);
        let event = CategoryProductsRemoved {
            category: *category,
            products: vec![product],
        };
        events.push(PendingEvent::new(subject(*category), &event));
    }
    batch.record_events(events);
    ports.commit(batch).await?;
    Ok(wanted.into_iter().collect())
}

/// Each of `products`' categories, sorted; a product in none is absent.
pub async fn categories_of_products<S: HasCategories>(
    ctx: &Ctx,
    ports: &S,
    products: &[ProductId],
) -> Result<HashMap<ProductId, Vec<CategoryId>>, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    Ok(ports.categories().categories_of(products).await?)
}

/// A page of the products directly in the category, newest first.
pub async fn list_category_products<S: HasCategories>(
    ctx: &Ctx,
    ports: &S,
    category: CategoryId,
    page: PageRequest<ProductId>,
) -> Result<Page<ProductId>, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    load(ports, category).await?;
    Ok(ports.categories().product_page(category, page).await?)
}
