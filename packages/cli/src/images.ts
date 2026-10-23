// The Docker images: the API, and the admin and storefront the deployment includes,
// built from this repository (`docker/*.Dockerfile`). On a server they're built by its
// own Docker over SSH (its architecture, no copying), or here for its platform and
// copied (`docker save | docker load`) when the server is too small to compile Rust.

import { join } from "node:path";

import { type AppEntry, imageTag, service } from "./apps.ts";
import type { App, Deployment } from "./deployment.ts";
import { type Machine, pipe, run } from "./shell.ts";
import { REPO } from "./templates.ts";

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
  return [
    {
      name: "api",
      tag: "hypha-api:current",
      dockerfile: join(REPO, "docker/api.Dockerfile"),
      context: REPO,
      args: hostCrate ? [`HOST_CRATE=${hostCrate}`] : [],
    },
    ...apps.map((app) => ({
      name: app,
      tag: `hypha-${app}:current`,
      dockerfile: join(REPO, "docker/app.Dockerfile"),
      context: REPO,
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
