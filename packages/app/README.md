# @hyphacommerce/app

The SDK for Hypha app authors. Part of [Hypha](https://github.com/hyphacommerce/hypha), the open-source commerce
API in Rust. Hypha is in public beta: this package follows the API's version and may change
between 0.x releases.

Manifest types, `verifySignature`/`signRequest` for `Hypha-Signature`, the OAuth client
with PKCE and rotating refresh tokens, `HyphaClient` (an admin client that refreshes its
token), `extensionHandler` for extension points, `webhookHandler` and `verifySessionToken`.
Web APIs only, so it runs on Node and on Cloudflare Workers alike.

See the apps guide: https://hyphacommerce.com/docs/apps.

## License

MIT or Apache-2.0, at your option.
