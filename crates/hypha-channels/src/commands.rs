//! Managing channels and publishing products to them (design note 0020). Channels need
//! `channels:*`; publishing is how a product is presented, so `products:write`.

use std::collections::BTreeSet;

use hypha_core::{
    AuthError, ChannelId, Ctx, ErrorCode, HasBatch, IdGenerator, PendingEvent, ProductId,
    RepoError, Scope, StageEvents, Subject,
};
use thiserror::Error;

use crate::{
    Channel, ChannelError, ChannelKind, ChannelRepo, HasChannels, ProductsPublished,
    ProductsUnpublished, StagePublications, events::CHANNEL, repo::keys,
};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Channel(#[from] ChannelError),
    #[error("channel {0} not found")]
    ChannelNotFound(ChannelId),
    #[error("a channel is already called {0:?}")]
    NameTaken(String),
    #[error(transparent)]
    Repo(#[from] RepoError),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Channel(e) => e.code(),
            CommandError::ChannelNotFound(_) => "channel_not_found",
            CommandError::NameTaken(_) => "channel_name_taken",
            CommandError::Repo(e) => e.code(),
        }
    }
}

/// A `Duplicate` on the name from the write is the same error as the check before it:
/// a race looks like a normal duplicate.
fn name_taken(e: RepoError, name: &str) -> CommandError {
    match e {
        RepoError::Duplicate(keys::CHANNEL_NAME) => CommandError::NameTaken(name.to_string()),
        e => e.into(),
    }
}

async fn name_in_use<R: ChannelRepo>(
    repo: &R,
    name: &str,
    except: Option<ChannelId>,
) -> Result<bool, RepoError> {
    let wanted = name.trim().to_lowercase();
    Ok(repo
        .list()
        .await?
        .iter()
        .any(|c| Some(c.id()) != except && c.name().to_lowercase() == wanted))
}

async fn load<R: ChannelRepo>(repo: &R, id: ChannelId) -> Result<Channel, CommandError> {
    repo.get(id).await?.ok_or(CommandError::ChannelNotFound(id))
}

pub struct CreateChannel {
    pub name: String,
    pub kind: ChannelKind,
}

pub async fn create_channel<S: HasChannels>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    input: CreateChannel,
) -> Result<Channel, CommandError> {
    ctx.require(Scope::ChannelsWrite)?;
    let mut channel = Channel::new(
        ChannelId::from_uuid(ids.new_uuid()),
        &input.name,
        input.kind,
    )?;
    if name_in_use(ports.channels(), channel.name(), None).await? {
        return Err(CommandError::NameTaken(channel.name().to_string()));
    }
    let name = channel.name().to_string();
    ports
        .channels()
        .insert(&mut channel)
        .await
        .map_err(|e| name_taken(e, &name))?;
    Ok(channel)
}

/// Changes only what's `Some`, like a JSON Merge Patch.
#[derive(Debug, Default)]
pub struct UpdateChannel {
    pub name: Option<String>,
}

pub async fn update_channel<S: HasChannels>(
    ctx: &Ctx,
    ports: &S,
    id: ChannelId,
    input: UpdateChannel,
) -> Result<Channel, CommandError> {
    ctx.require(Scope::ChannelsWrite)?;
    let mut channel = load(ports.channels(), id).await?;
    if let Some(name) = input.name {
        channel.rename(&name)?;
        if name_in_use(ports.channels(), channel.name(), Some(id)).await? {
            return Err(CommandError::NameTaken(channel.name().to_string()));
        }
    }
    let name = channel.name().to_string();
    ports
        .channels()
        .update(&mut channel)
        .await
        .map_err(|e| name_taken(e, &name))?;
    Ok(channel)
}

pub async fn get_channel<S: HasChannels>(
    ctx: &Ctx,
    ports: &S,
    id: ChannelId,
) -> Result<Channel, CommandError> {
    ctx.require(Scope::ChannelsRead)?;
    load(ports.channels(), id).await
}

pub async fn list_channels<S: HasChannels>(
    ctx: &Ctx,
    ports: &S,
) -> Result<Vec<Channel>, CommandError> {
    ctx.require(Scope::ChannelsRead)?;
    Ok(ports.channels().list().await?)
}

fn subject(channel: ChannelId) -> Subject {
    Subject::new(CHANNEL, channel.as_uuid())
}

/// Each once, in order.
fn distinct(products: Vec<ProductId>) -> Vec<ProductId> {
    let mut seen = BTreeSet::new();
    products.into_iter().filter(|p| seen.insert(*p)).collect()
}

/// Publishes products to the channel. Product IDs aren't checked (a leaf crate can't
/// see products): an unknown one is stored and never matches. Already published ones
/// change nothing and aren't in the event.
pub async fn publish_products<S>(
    ctx: &Ctx,
    ports: &S,
    channel: ChannelId,
    products: Vec<ProductId>,
) -> Result<(), CommandError>
where
    S: HasChannels + HasBatch<Batch: StagePublications + StageEvents>,
{
    ctx.require(Scope::ProductsWrite)?;
    load(ports.channels(), channel).await?;
    let products = distinct(products);
    let already = ports.channels().published(channel, &products).await?;
    let new: Vec<ProductId> = products
        .into_iter()
        .filter(|p| !already.contains(p))
        .collect();
    if new.is_empty() {
        return Ok(());
    }
    let mut batch = ports.batch();
    batch.publish_products(channel, &new);
    let event = ProductsPublished {
        channel,
        products: new,
    };
    batch.record_events(vec![PendingEvent::new(subject(channel), &event)]);
    Ok(ports.commit(batch).await?)
}

/// Unpublishes products from the channel; ones not published change nothing.
pub async fn unpublish_products<S>(
    ctx: &Ctx,
    ports: &S,
    channel: ChannelId,
    products: Vec<ProductId>,
) -> Result<(), CommandError>
where
    S: HasChannels + HasBatch<Batch: StagePublications + StageEvents>,
{
    ctx.require(Scope::ProductsWrite)?;
    load(ports.channels(), channel).await?;
    let products = distinct(products);
    let published = ports.channels().published(channel, &products).await?;
    let gone: Vec<ProductId> = products
        .into_iter()
        .filter(|p| published.contains(p))
        .collect();
    if gone.is_empty() {
        return Ok(());
    }
    let mut batch = ports.batch();
    batch.unpublish_products(channel, &gone);
    let event = ProductsUnpublished {
        channel,
        products: gone,
    };
    batch.record_events(vec![PendingEvent::new(subject(channel), &event)]);
    Ok(ports.commit(batch).await?)
}

/// The channels a product is published to, sorted.
pub async fn product_channels<S: HasChannels>(
    ctx: &Ctx,
    ports: &S,
    product: ProductId,
) -> Result<Vec<ChannelId>, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    Ok(ports
        .channels()
        .channels_of(&[product])
        .await?
        .remove(&product)
        .unwrap_or_default())
}

/// Makes `channels` the product's channels (the product page's Publishing card):
/// publishes to the new ones and unpublishes from the others, in one batch with an
/// event per channel. Every channel must exist (`ChannelNotFound`, from the body).
pub async fn set_product_channels<S>(
    ctx: &Ctx,
    ports: &S,
    product: ProductId,
    channels: Vec<ChannelId>,
) -> Result<Vec<ChannelId>, CommandError>
where
    S: HasChannels + HasBatch<Batch: StagePublications + StageEvents>,
{
    ctx.require(Scope::ProductsWrite)?;
    let all: BTreeSet<ChannelId> = ports
        .channels()
        .list()
        .await?
        .iter()
        .map(Channel::id)
        .collect();
    let wanted: BTreeSet<ChannelId> = channels.into_iter().collect();
    if let Some(unknown) = wanted.iter().find(|c| !all.contains(c)) {
        return Err(CommandError::ChannelNotFound(*unknown));
    }
    let current: BTreeSet<ChannelId> = ports
        .channels()
        .channels_of(&[product])
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
    for channel in wanted.difference(&current) {
        batch.publish_products(*channel, &[product]);
        events.push(PendingEvent::new(
            subject(*channel),
            &ProductsPublished {
                channel: *channel,
                products: vec![product],
            },
        ));
    }
    for channel in current.difference(&wanted) {
        batch.unpublish_products(*channel, &[product]);
        events.push(PendingEvent::new(
            subject(*channel),
            &ProductsUnpublished {
                channel: *channel,
                products: vec![product],
            },
        ));
    }
    batch.record_events(events);
    ports.commit(batch).await?;
    Ok(wanted.into_iter().collect())
}

/// Whether `product` is sold through `channel`: for the storefront's checks (carts,
/// product reads), one query.
pub async fn is_published<S: HasChannels>(
    ports: &S,
    channel: ChannelId,
    product: ProductId,
) -> Result<bool, RepoError> {
    Ok(ports
        .channels()
        .published(channel, &[product])
        .await?
        .contains(&product))
}
