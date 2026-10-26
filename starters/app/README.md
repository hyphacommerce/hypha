# A Hypha app

Made by `hypha create app`. It's an app on [Hypha's apps platform](https://hyphacommerce.com/docs/guides/apps/):
installed from a store's admin over OAuth, told about events by webhooks, and called at
checkout through the `shipping.rates` extension point, where it quotes a flat rate.

```
pnpm install
pnpm test
APP_URL=http://localhost:8787 pnpm dev
```

Then in your Hypha admin: Apps → Install app → `http://localhost:8787`. A local API must be
allowed to call private addresses (`outbound.allow_private = true` in its `hypha.config.toml`,
or `HYPHA_ALLOW_PRIVATE_CALLBACKS=true`).

- `src/manifest.ts`: the key, name, scopes, webhooks and extension points. Change these first.
- `src/installations.ts`: where each store's tokens and secret live. In memory here: give it a
  database before anyone installs the app for real.
- `src/rates.ts`: the extension handler. Add one `defineExtensionHandler` per point you answer
  and list it in `src/app.ts`.
- `src/node.ts`, `src/worker.ts`: the hosts; `wrangler.toml.example` and the `Dockerfile` deploy
  them. Once it keeps installations in a database (D1 on Workers, a volume on a server),
  `hypha apps add <this repository's URL>` deploys the app beside a Hypha deployment.

The SDK is `@hyphacommerce/app`; the typed admin API comes with it (`client.admin`). The
[extension points reference](https://hyphacommerce.com/docs/api/extensions/) lists every
point's request and response.
