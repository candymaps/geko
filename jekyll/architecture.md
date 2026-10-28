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

`geko.toml` declares the scope and the modules. geko downloads the modules, asks each one to download and
build its part for the scope, publishes the bundle and serves it. See
[Data and bundles]({% link data-and-bundles.md %}).

## Two nodes, if you want

Building and serving can run on **different machines**: building needs a lot of RAM and CPU for minutes
or hours, and serving needs little.

```
 build node
   geko.toml -> active modules -> open each one (linked, GEKO_MODULES or signed download)
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
- **A module that is disabled costs nothing:** it is not downloaded, not instantiated, opens no files and
  registers no routes.
- **Origin of a module**, in this order: linked into the binary (none, in the binary of this repository; a
  module can link itself with `geko_app::main` for its own checks); a folder of `GEKO_MODULES`, loaded as a
  library (`kind = "dylib"`); or downloaded and signed from `ghcr.io/candymaps/geko-<module>`.
- **Partial bundles.** The manifest of a bundle says which modules it holds. `geko serve` uses the
  version of each module that built the bundle it serves, and downloads it if missing.

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
  Dockerfile, develop/       image ghcr.io/candymaps/geko, without modules; make start-dev
  ci/                        test.sh (checks), affected.sh (which ones a change needs)
  crates/
    geko-module-sdk/         the contract (Module), shared types, bundle, stages, conformance
    geko-fetch/              HTTP Downloader, raw cache, OCI registry client
    geko-artifact/           signed artifacts: modules and data packages; geko-module-sign
    geko-server/             common HTTP, mounting of the modules, bundle reload
    geko-app/                the host: binary and library, CLI, configuration, adapters, publication
```

`geko-module-sdk`, `geko-fetch` and `geko-artifact` are the git dependencies of the modules, at the tag
`sdk-<contract>`.

## Versions and compatibility

- **The host fixes the contract, not the version of the module.** Each release of the host declares the
  contracts it understands. With a module active, the host downloads the newest one of the highest
  contract it understands: a fix to the geocoder arrives without a release of the host.
- **A pinned version** is optional: `[modules.geocoder] version = "1.4.2"` (production, going back).
- **The bundle fixes the version it is served with.** Its manifest keeps the exact version of the module
  that built it, and `geko serve` uses that one.
- **A version that does not fit the host** is an error in `plan`, `fetch`, `build`, `up` and `serve`,
  before touching any source, with the compatible versions and the way out: remove `version` to use the
  newest compatible one. geko never chooses another one silently.
- `geko plan` shows the version of each module, its contract and whether it is pinned or the newest
  compatible one.

## Platforms

Production serving is on Linux. Development can be done on Windows or WSL.
