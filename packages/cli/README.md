# @hyphacommerce/cli

Hosting [Hypha](https://github.com/hyphacommerce/hypha), the open-source commerce API in
Rust. Hypha is in public beta: this CLI follows the API's version and may change between
0.x releases.

```bash
npx @hyphacommerce/cli init      # writes a deployment folder: settings, secrets, compose files
npx @hyphacommerce/cli deploy    # runs it on a server over SSH, or on this machine
```

`init` asks for the target (a server, this machine, or Cloudflare Workers), the domains
and the owner's account, and writes `hypha-deploy/`. `deploy` pulls the API, admin and
storefront images published with this version (`ghcr.io/hyphacommerce/hypha-*`), copies
the folder to the server and starts Postgres, file storage, the API, the apps and Caddy
with HTTPS. `status` and `logs [service]` look at it; `apps` and `spores` manage what's
deployed beside the API.

Building the images from source (`--build-from-source`), spores, co-located apps and the
Cloudflare target need a checkout of the repository: run `pnpm hypha` from it.

The guide: https://hyphacommerce.com/docs/deploy.

## License

MIT or Apache-2.0, at your option.
