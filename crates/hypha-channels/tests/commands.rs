//! Channels and publishing (design note 0020) over a store in memory.

use hypha_channels::{
    ChannelKind, CommandError, CreateChannel, UpdateChannel, create_channel, list_channels,
    product_channels, publish_products, set_product_channels, unpublish_products, update_channel,
};
use hypha_core::{Actor, AuthError, ChannelId, Ctx, ProductId, Scope, UserId};
use hypha_testkit::{InMemoryPorts, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

fn pid(n: u128) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(n))
}

struct World {
    ports: InMemoryPorts,
    ids: SequentialIds,
    ctx: Ctx,
}

impl World {
    fn new() -> Self {
        Self {
            ports: InMemoryPorts::default(),
            ids: SequentialIds::default(),
            ctx: Ctx::system(),
        }
    }

    fn create(&self, name: &str) -> Result<ChannelId, CommandError> {
        let input = CreateChannel {
            name: name.to_string(),
            kind: ChannelKind::Custom,
        };
        block_on(create_channel(&self.ctx, &self.ports, &self.ids, input)).map(|c| c.id())
    }

    /// The events of `event_type` recorded, as their data.
    fn events(&self, event_type: &str) -> Vec<serde_json::Value> {
        self.ports
            .outbox
            .events()
            .into_iter()
            .filter(|e| e.event_type == event_type)
            .map(|e| e.data)
            .collect()
    }
}

#[test]
fn names_are_unique_ignoring_case() {
    let w = World::new();
    let web = w.create("Web").unwrap();
    w.create("Outlet").unwrap();
    assert_eq!(
        w.create(" web "),
        Err(CommandError::NameTaken("web".to_string()))
    );
    let rename = UpdateChannel {
        name: Some("OUTLET".to_string()),
    };
    assert_eq!(
        block_on(update_channel(&w.ctx, &w.ports, web, rename)).map(|_| ()),
        Err(CommandError::NameTaken("OUTLET".to_string()))
    );
    // Its own name in another case is fine.
    let rename = UpdateChannel {
        name: Some("WEB".to_string()),
    };
    assert!(block_on(update_channel(&w.ctx, &w.ports, web, rename)).is_ok());
    assert_eq!(block_on(list_channels(&w.ctx, &w.ports)).unwrap().len(), 2);
}

#[test]
fn publishing_records_only_what_changed() {
    let w = World::new();
    let web = w.create("Web").unwrap();
    let publish = |products: Vec<ProductId>| {
        block_on(publish_products(&w.ctx, &w.ports, web, products)).unwrap()
    };
    publish(vec![pid(1), pid(2), pid(1)]);
    publish(vec![pid(2), pid(3)]);
    publish(vec![pid(3)]);
    let published = w.events("channel.products_published");
    assert_eq!(published.len(), 2, "the last publishes nothing new");
    assert_eq!(published[0]["products"].as_array().unwrap().len(), 2);
    assert_eq!(published[1]["products"], serde_json::json!([pid(3)]));

    block_on(unpublish_products(
        &w.ctx,
        &w.ports,
        web,
        vec![pid(1), pid(9)],
    ))
    .unwrap();
    assert_eq!(
        w.events("channel.products_unpublished")[0]["products"],
        serde_json::json!([pid(1)])
    );
    assert_eq!(
        block_on(product_channels(&w.ctx, &w.ports, pid(1))),
        Ok(vec![])
    );
}

#[test]
fn a_products_channels_are_replaced_in_one_batch() {
    let w = World::new();
    let web = w.create("Web").unwrap();
    let agents = w.create("Agents").unwrap();
    block_on(publish_products(&w.ctx, &w.ports, web, vec![pid(1)])).unwrap();

    let set = block_on(set_product_channels(&w.ctx, &w.ports, pid(1), vec![agents]));
    assert_eq!(set, Ok(vec![agents]));
    assert_eq!(
        block_on(product_channels(&w.ctx, &w.ports, pid(1))),
        Ok(vec![agents])
    );
    assert_eq!(w.events("channel.products_published").len(), 2);
    assert_eq!(w.events("channel.products_unpublished").len(), 1);

    // The same set again writes nothing.
    let before = w.ports.outbox.events().len();
    block_on(set_product_channels(&w.ctx, &w.ports, pid(1), vec![agents])).unwrap();
    assert_eq!(w.ports.outbox.events().len(), before);

    let unknown = ChannelId::from_uuid(Uuid::from_u128(999));
    assert_eq!(
        block_on(set_product_channels(
            &w.ctx,
            &w.ports,
            pid(1),
            vec![unknown]
        )),
        Err(CommandError::ChannelNotFound(unknown))
    );
}

#[test]
fn channels_need_channel_scopes_and_publishing_products_write() {
    let w = World::new();
    let web = w.create("Web").unwrap();
    let staff = |scopes| Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(5))), scopes);
    let missing = |scope| Err(CommandError::Auth(AuthError::MissingScope(scope)));

    let reader = staff(vec![Scope::ChannelsRead, Scope::ProductsRead]);
    assert!(block_on(list_channels(&reader, &w.ports)).is_ok());
    let input = CreateChannel {
        name: "Outlet".to_string(),
        kind: ChannelKind::Custom,
    };
    assert_eq!(
        block_on(create_channel(&reader, &w.ports, &w.ids, input)).map(|_| ()),
        missing(Scope::ChannelsWrite)
    );
    assert_eq!(
        block_on(publish_products(&reader, &w.ports, web, vec![pid(1)])),
        missing(Scope::ProductsWrite)
    );
    // Channel management doesn't publish.
    let manager = staff(vec![Scope::ChannelsWrite]);
    assert_eq!(
        block_on(publish_products(&manager, &w.ports, web, vec![pid(1)])),
        missing(Scope::ProductsWrite)
    );
}
