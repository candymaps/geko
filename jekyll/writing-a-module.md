---
title: Writing a module
parent: Modules
nav_order: 3
---

# Writing a module
{: .no_toc }

A module is a package that the host downloads and loads at run time. It lives in its own repository, is
published at its own pace, and meets the contract of `geko-module-sdk`. The
[geocoder]({% link geocoder.md %}) and [tiles]({% link tiles.md %}) are the two existing examples.

1. TOC
{:toc}

## The SDK

The SDK is defined and published by the host, in `candymaps/geko`: the port belongs to whoever consumes
it. GitHub Packages has no Cargo registry, so it is published as a git tag of the host repository,
`sdk-<contract>`, and the module uses it as a git dependency:

```toml
geko-module-sdk = { git = "https://github.com/candymaps/geko", tag = "sdk-1" }
```

If the repository of the host is private, the CI of the module needs a read token.

## What a module implements

The `Module` trait, in `crates/geko-module-sdk/src/module.rs`, has these parts:

| Part | What it is for |
|---|---|
| `describe` | Name, version, contract, platform, optional capabilities, the HTTP routes it serves (method, pattern, whether they go to the search pool, whether they are public without an API key), its home page, its `openapi.yaml`, what it adds to `/v1/attribution`, and its commands. |
| `plan`, `fetch` | From the scope and its configuration to the data that is needed, downloaded with the `Downloader` that the host offers (cache, `offline`, licenses, parallel downloads). |
| `prepare` | What the commands that build need (the geocoder downloads here the country packs of the scope). |
| `build` | Its stages per country, with their stamps in its own work directory, and the parts of the bundle it leaves, their licenses and metadata. The host publishes them as opaque files (path, hash, size). Afterwards, `published` and `next_build` (when to build again in `geko up`). |
| `load(bundle)` | Opens its part of the bundle. |
| `handle(Request) -> Response` | Serves a request. It is synchronous: the host mounts axum, applies access and limits, and calls from its pool. |
| `metrics(&mut String)`, `shutdown` | Prometheus metrics and shutdown. |
| `command(args)` | Commands of the module (`geko geocoder query …`), forwarded as they are. |
| `purge` | Its data that the configuration no longer asks for (`geko purge data`), and its packages outside the scope (`geko purge packages`). |

The host passes to the module the content of its file (`geko-<module>.toml`) without interpreting it. The
module validates it, with errors that name the file and the key.

### Routes

A module declares relative routes (`v1/search`) and the host always mounts them under `/<module>/`. The
common routes (`/health`, `/metrics`, `/v1/attribution`, `/docs`) are the host's.

### What does not touch the host

A change in the module (ranking, index format, a new source, a new route, a new option, a metric, a
debugging command, the data of a pack) does not touch the host. Only a new capability of the contract, or
an incompatible change of the SDK, does. Changes of the contract are additive whenever possible: optional
fields, and capabilities that are queried in `describe` before they are used.

## Packaging

The module is built as a library (`.so`) that the host loads with `dlopen`. The border is a minimal
`extern "C"` interface: the `.so` exports five functions with `geko_module_sdk::export_module!`, which
pass serialized messages, with no Rust types across. Each function runs inside `catch_unwind`, and each
side frees its own memory. So the host and the module can be built with different Rust versions.

- Build the library in a separate crate, so that linking the module into the host does not also build the
  `cdylib`.
- Declare `mimalloc` in the library of the module. Without it, the module loses what mimalloc gained.
- A package has a `module.toml` with `kind = "dylib"`, `name`, `version`, `contract`, `platform` and
  `entry` (the library).

### Trying it locally

`GEKO_MODULES` is a directory with one folder per module (`<module>/module.toml`). It replaces linking
and downloading, and a module that is not there is an error, never a download.

A module can also be linked into the host for its own checks (the golden, valgrind), through
`geko_app::main(linked)`, without the host depending on it.

### Publishing

A module publishes itself from its own repository, as a signed OCI artifact in
`ghcr.io/candymaps/geko-<module>`:

- tags `<version>-<platform>` and `contract-<n>-<platform>` (the latter points to the newest module of that
  contract), and the annotations `geko.version`, `geko.contract` and `geko.platform`;
- the version is `<version>-<commit>`, or `<version>` in a release, and it is compiled in with
  `GEKO_MODULE_VERSION` (`module_version!` of the SDK);
- the `module.toml` and the package are signed with `geko-module-sign` (`geko-artifact`).

Before downloading anything, the host reads only the annotations of a version: contract and platform. If
they do not fit, it fails before touching the sources. After the download it checks the signature, and on
opening, the greeting of the adapter, which is the first message and carries the contract. See
[Security and licenses]({% link security.md %}).

## The conformance suite

Every module must pass it. It is in `geko-module-sdk`, with the `conformance` feature:

```rust
geko_module_sdk::conformance::module::check
```

It runs over a module opened with an empty scope and no bundle, and checks that:

- `describe` carries the contract of the SDK, a version, and valid routes with no repeats;
- its `openapi` is one of its GET routes;
- commands do not repeat, and an unknown one is an error;
- no progress is reported outside a build;
- `/metrics` is in Prometheus format, under `geko_`;
- every route, with strange requests (empty, impossible values, 10,000 letters, nulls, an id with `..`, a
  broken body), answers without a 500, and an error is `{"error": …}` in JSON, also after a bundle that
  lacks its part, and from 8 threads at once.

A new requirement of the contract goes into this suite.

## Changing the contract

The contract is small and additive. A change that breaks it raises `CONTRACT` (in `module.rs`), and every
module listed in `ci/modules` must follow before it is merged:

1. Open the pull request of the host.
2. Point each module to its commit (`rev`).
3. Publish each module, and run the checks of the pull request again: `ci/modules.sh check` must pass.
4. Merge it with a **merge commit**. A squash would orphan the `rev` of the modules.
5. Move the modules to the new tag `sdk-<contract>`.

Neither `dev` nor `main` accepts a contract that a module does not speak yet. The host does not publish
its image, its tag or its release until a module is published for each contract it declares.
