---
title: Architecture
nav_order: 2
---

# Architecture
{: .no_toc }

geko is a **host** and a set of **modules**. The host is the `geko` binary: it reads the configuration,
resolves the scope, downloads and loads the modules, publishes and reloads the bundles and serves what
is common over HTTP. Each service is a module, with its own repository, which the host knows only through
the contract of `geko-module-sdk`.

1. TOC
{:toc}

## The whole cycle is automatic

`geko.toml` declares the scope and the modules. geko loads the modules of its version, asks each one to download and
build its part for the scope, publishes the bundle and serves it. See
[Data and bundles]({% link data-and-bundles.md %}).

## Two nodes, if you want

Building and serving can run on **different machines**: building needs a lot of RAM and CPU for minutes
or hours, and serving needs little.

```
 build node
   geko.toml -> active modules -> open each one (linked or GEKO_MODULES)
             -> plan / fetch / build of each module (the Downloader of the host,
                its part of the bundle)
             -> bundle vN/ (the parts of the modules + manifest.json) -> publication
                         |
                         |  downloads only what changed
                         v
 serve node
   reload: download, validate, load each module, atomic switch
   HTTP: /health, /metrics, /v1/attribution, /docs, API keys, CORS, limits
         `-> /<module>/...  ->  Module::handle
```

## Modules

- **Independence.** The host depends on no module, and no module depends on another. What they share goes
  into the SDK. If a type, a piece of data or a port only makes sense for a module, it lives in the
  module.
- **Routes under the name of the module** (`/geocoder/…`, `/tiles/…`): two modules cannot step on each
  other, and a new one does not touch the routes of the others.
- **Own configuration**, `geko-<module>.toml`, beside `geko.toml`: the host does not read it, it passes it
  on.
- **A module that is disabled costs nothing:** it is not instantiated, opens no files and
  registers no routes.
- **Origin of a module**, in this order: linked into the binary (none, in the binary of this repository; a
  module can link itself with `geko_app::main` for its own checks); a folder of `GEKO_MODULES`, loaded as a
  library (`kind = "dylib"`). The image of geko carries the signed modules of its `modules.lock` and sets
  `GEKO_MODULES`; geko downloads none.
- **Partial bundles.** The manifest of a bundle says which modules it holds.

To write one, see [Writing a module]({% link writing-a-module.md %}).

## One process

Modules are loaded as libraries (`DylibModule`) into the process of geko. Measured with all of Spain and
16 concurrent requests, it costs nothing beyond noise, while running a module as a separate process lost
19 % of throughput in search, 20 % in autocomplete and 55 % in reverse, so it is the only adapter.

The trade-off: a panic only fails its request (`catch_unwind` at the border), but a serious failure of a
module (a segfault, a stack overflow, running out of memory) takes geko down with it, and a new version of
a module needs a restart. To isolate modules, use one process per module:
[Kubernetes]({% link kubernetes.md %}).

## Ports

Only where there is a realistic alternative, or the technical decision is open, is something abstracted:
a trait designed before it has two implementations tends to copy the shape of the first one.

| Port | Responsibility | Implementations |
|---|---|---|
| `Module` | The contract of a module: describe itself, plan, download, build its part of the bundle, load it, serve requests, metrics, commands | `DylibModule` (a library loaded into the process); `StaticModule` (linked, for tests) |
| `Downloader` | Download files with cache, resume, ETag, retries and checksums | `geko-fetch` (HTTP); fake downloaders in tests |

Everything else (HTTP transport, bundle storage, module registry) is plain code: its trait is extracted
when a second real implementation appears. No concrete technology (`ureq`, `axum`, `libloading`…) leaves
its crate. Each port has a conformance suite that every implementation must pass.

## The scope

The scope is declared with ISO 3166-1 (country) and ISO 3166-2 (region) codes. The host only validates the
codes and passes them to the modules; what a region means for each one is up to the module. A new country
is a data pack of the module, with no change to the engine.

## The repository

```
geko/
  openapi.yaml               routes of the host
  Dockerfile, develop/       image ghcr.io/candymaps/geko, with the modules of modules.lock; make start-dev
  ci/                        test.sh (checks), affected.sh (which ones a change needs)
  crates/
    geko-module-sdk/         the contract (Module), shared types, bundle, stages, conformance
    geko-fetch/              HTTP Downloader, raw cache, OCI registry client
    geko-artifact/           signed artifacts: modules and data packages; geko-module-sign
    geko-server/             common HTTP, mounting of the modules, bundle reload
    geko-app/                the host: binary and library, CLI, configuration, adapters, publication
```

`geko-module-sdk`, `geko-fetch` and `geko-artifact` are the git dependencies of the modules, at the tag
`sdk-v<version>`.

## Versions and compatibility

- **The version of the host decides the modules.** `modules.lock` fixes the exact package of each one
  (version, platform and digest), and the image carries them. Raising a module means raising
  `modules.lock` (a daily workflow proposes the pull request) and publishing the host.
- **Compatibility is checked when publishing,** not when loading: the checks run the real modules of
  `modules.lock` through the conformance suite and a build and serve of Luxembourg, and nothing is
  published until they pass. The host compares no version when it loads a bundle.
- **The SDK has its own version** (`sdk-v<version>`), the same in its three crates. A module moves to a
  new tag when it needs the change.
- **Each module judges its own part of a bundle.** Its manifest keeps the exact version of the module that
  built it, as a record: `geko serve` does not compare it. The module loads what it can read, and rejects
  the rest. One that rejects a bundle goes on with the one it had, and the others load it.
- `geko plan` shows the version of each active module and where it comes from.

## Platforms

Production serving is on Linux. Development can be done on Windows or WSL.
