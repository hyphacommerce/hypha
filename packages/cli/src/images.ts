// The Docker images: the API, and the admin and storefront the deployment includes.
// By default the published ones, pinned to this CLI's version (a release publishes the
// CLI and the images together), which the server pulls. Built from source instead
// (`docker/*.Dockerfile`, a checkout) when asked, or when the deployment has spores,
// since its API is then its own: on the server by its own Docker over SSH (its
// architecture, no copying), or here for its platform and copied (`docker save | docker
// load`) when the server is too small to compile Rust.

import { join } from "node:path";

import { type AppEntry, imageTag, service } from "./apps.ts";
import type { App, Deployment, EnvFile } from "./deployment.ts";
import { type Machine, pipe, run } from "./shell.ts";
import { requireRepo } from "./templates.ts";
import { VERSION } from "./version.ts";

/** Where the release workflow publishes the images. */
export const REGISTRY = "ghcr.io/hyphacommerce";

/** `.env`'s image references, read by `compose.yaml`; without them it runs the
 * `:current` images a source build leaves in Docker. */
export const IMAGE_VARS: Record<App | "api", string> = {
  api: "HYPHA_API_IMAGE",
  admin: "HYPHA_ADMIN_IMAGE",
  storefront: "HYPHA_STOREFRONT_IMAGE",
};

/** The published image for a service at a version: `ghcr.io/hyphacommerce/hypha-api:0.1.0`. */
export function publishedImage(name: App | "api", version = VERSION): string {
  return `${REGISTRY}/hypha-${name}:${version}`;
}

/** Points `.env` at the published images at `version` (every service, so a profile
 * turned on later gets its image too), or at the source build's. */
export function setImageRefs(env: EnvFile, source: boolean, version = VERSION): void {
  for (const name of ["api", "admin", "storefront"] as const) {
    if (source) env.delete(IMAGE_VARS[name]);
    else env.set(IMAGE_VARS[name], publishedImage(name, version));
  }
}

/** An image: its Dockerfile and build context (absolute paths). */
export type Image = {
  name: string;
  tag: string;
  dockerfile: string;
  context: string;
  app?: App;
  /** More `--build-arg`s (`HOST_CRATE=…`). */
  args?: string[];
};

/** The images: the API (from `hostCrate`, the deployment's own host crate, when it has
 * spores), and the apps. */
export function images(apps: App[], hostCrate?: string): Image[] {
  const repo = requireRepo("Building the images from source");
  return [
    {
      name: "api",
      tag: "hypha-api:current",
      dockerfile: join(repo, "docker/api.Dockerfile"),
      context: repo,
      args: hostCrate ? [`HOST_CRATE=${hostCrate}`] : [],
    },
    ...apps.map((app) => ({
      name: app,
      tag: `hypha-${app}:current`,
      dockerfile: join(repo, "docker/app.Dockerfile"),
      context: repo,
      app,
    })),
  ];
}

/** A co-located app's image: its own Dockerfile, in its clone, which holds the Node bundle
 * built here (`apps.ts`). */
export function appImage(entry: AppEntry, clone: string): Image {
  return {
    name: service(entry),
    tag: imageTag(entry),
    dockerfile: join(clone, "Dockerfile"),
    context: clone,
  };
}

export function buildArgs(image: Image): string[] {
  return [
    "build",
    "-f",
    image.dockerfile,
    "-t",
    image.tag,
    ...(image.app ? ["--build-arg", `APP=${image.app}`] : []),
    ...(image.args ?? []).flatMap((arg) => ["--build-arg", arg]),
    image.context,
  ];
}

/** The server's platform for `docker build --platform`, from `uname -m`. */
async function platform(machine: Machine): Promise<string> {
  const arch = (await machine.sh("uname -m", { capture: true })).trim();
  if (arch === "x86_64" || arch === "amd64") return "linux/amd64";
  if (arch === "aarch64" || arch === "arm64") return "linux/arm64";
  throw new Error(`the server's architecture, ${arch}, has no build here`);
}

/** Builds `image` where the deployment says, leaving it in the machine's Docker. */
export async function build(d: Deployment, machine: Machine, image: Image): Promise<void> {
  if (!machine.ssh) {
    await run("docker", buildArgs(image));
  } else if (d.buildOn === "server") {
    await run("docker", ["--host", `ssh://${machine.ssh}`, ...buildArgs(image)]);
  } else {
    const [, ...rest] = buildArgs(image);
    await run("docker", [
      "buildx",
      "build",
      "--platform",
      await platform(machine),
      "--load",
      ...rest,
    ]);
    await pipe(
      ["docker", ["save", image.tag]],
      ["ssh", ["-o", "BatchMode=yes", machine.ssh, "docker load"]],
    );
  }
}
