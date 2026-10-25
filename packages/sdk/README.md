# @hyphacommerce/sdk

Typed clients for Hypha's admin and storefront APIs. Part of [Hypha](https://github.com/hyphacommerce/hypha), the open-source commerce
API in Rust. Hypha is in public beta: this package follows the API's version and may change
between 0.x releases.

```ts
import { storefrontClient } from "@hyphacommerce/sdk/storefront";

const api = storefrontClient({ baseUrl: "https://api.example.com", publishableKey: "hypha_pk_…" });
const { data, error } = await api.GET("/storefront/products", { params: { query: { country: "DE" } } });
```

The types are generated from the API's OpenAPI documents (`openapi/*.json` in the
repository), so a release of this package matches the API release with the same version.
`@hyphacommerce/sdk/admin` is the admin surface (staff sessions and secret keys: server
side only).

## License

MIT or Apache-2.0, at your option.
