<p align="center">
  <img height="100" src="https://placehold.co/100x30/transparent/A4CC47?text=Geko&font=raleway" alt="geko">
</p>
<h3 align="center">A modular, multi-country geospatial engine that is easy to configure.</h3>
<p align="center">geko automatically downloads the data for each configured scope and serves it with outstanding efficiency.</p>
<p align="center">
  <a href="https://github.com/candymaps/geko/releases/latest"><img src="https://img.shields.io/github/v/release/candymaps/geko?label=version&color=blue&style=plastic" alt="Version"></a>
  <a href="https://www.gnu.org/licenses/agpl-3.0"><img src="https://img.shields.io/badge/license-AGPL--3.0-orange?style=plastic" alt="License"></a>
  <a href="https://github.com/orgs/candymaps/packages?repo_name=geko"><img src="https://img.shields.io/badge/docker-ghcr.io-informational?style=plastic&logo=docker" alt="Docker"></a>
  <a href="https://github.com/candymaps/geko/actions/workflows/publish.yml"><img src="https://github.com/candymaps/geko/actions/workflows/publish.yml/badge.svg?branch=main" alt="publish"></a>
</p>
<p align="center">
  <a href="#installation-and-usage"><b>🚀 Installation</b></a>&nbsp;&nbsp;&nbsp;∣&nbsp;&nbsp;&nbsp;
  <a href="#"><b>🗺️ Demo</b></a>&nbsp;&nbsp;&nbsp;∣&nbsp;&nbsp;&nbsp;
  <a href="#"><b>📖 Documentation</b></a>&nbsp;&nbsp;&nbsp;∣&nbsp;&nbsp;&nbsp;
  <a href="#"><b>⚡ API</b></a>
</p>

# geko

A modular, multi-country geospatial engine. Each service is a module, enabled in `geko.toml`, on top of the same data:

- **Geocoding** (available, [geko-geocoder](https://github.com/candymaps/geko-geocoder)): turns addresses and place names into coordinates (search and autocomplete), and coordinates into addresses (reverse).
- **Vector tiles** (available, [geko-tiles](https://github.com/candymaps/geko-tiles)): vector tiles of the scope, with the Protomaps styles, their fonts and sprites.
- **Routing…** (planned): new modules that plug in without changing the others.

They all share the same foundation:

- **A single source of truth:** OpenStreetMap, Overture Maps and each country's official sources are merged into one entity per street, address or place, with no duplicates.
- **Fully automatic:** `geko.toml` declares the modules and the countries and regions to serve; geko downloads only what those modules need, builds their indexes, serves them and keeps them up to date without restarting.
- **Fast and lightweight:** written in Rust, with its own indexes. Geocoding answers in milliseconds and serves all of Spain with less than 1 GB of memory.
- **A new country is a data pack**, with no changes to the engine.

This repository is the host: the `geko` binary, which downloads the modules, builds and reloads their bundles and serves what they share. Each module lives in its own repository. The specification of the host is in [PROJECT.md](docs/PROJECT.md), and the contract of the modules in [MODULES_PLAN.md](docs/MODULES_PLAN.md) (both in Spanish).

## Installation and usage

All you need is Docker.

**1. A `geko.toml`** with the modules and countries to serve:

```toml
[modules]
geocoder = true          # geocoding
tiles    = true          # vector tiles; more modules (routing…) are on the way

[[scope]]
country = "ES"            # ISO 3166-1; one [[scope]] per country (LU…)
# regions = ["ES-PM"]     # only part of it: ISO 3166-2, from the regions in the country pack (provinces or autonomous communities, in Spain); without regions, the whole country

[data]
accept_licenses = true    # you accept the licenses of the sources: ODbL (OSM), CDLA (Overture), CC BY (CartoCiudad)
```

Every section and option of geko itself is in [Configuration reference](#configuration-reference-gekotoml), below, with its default value and what it does. Each module has its own file beside it, `geko-<module>.toml`, documented in the README of the module: the geocoder's (its sources, time budgets, cache…) in [geko-geocoder](https://github.com/candymaps/geko-geocoder#configuration-geko-geocodertoml). Without it, a module takes its defaults.

The image carries no configuration: without a mounted `geko.toml`, it stops right away asking for one. Inside the container, geko always listens on `0.0.0.0:8080`, whatever `[serve] listen` says.

**2. Start it:**

```bash
docker run -d --name geko -p 8080:8080 -v geko-data:/var/lib/geko \
  -v "$PWD/geko.toml:/etc/geko/geko.toml:ro" ghcr.io/candymaps/geko
```

To change the options of a module, mount its file too, beside it (`-v "$PWD/geko-geocoder.toml:/etc/geko/geko-geocoder.toml:ro"`).

The first time, it downloads the data for those countries, builds the indexes and serves them (it answers 503 until then): about 2 minutes for Luxembourg, around an hour for all of Spain (about 8 GB of disk while building and 5 GB afterwards, and 3 GB of memory). Follow it with `docker logs -f geko`: while it builds, one line every 2 seconds with how far it has gone (with `GEKO_VERBOSE=1`, `-e GEKO_VERBOSE=1` in Docker, or `geko --verbose`, every step instead). Later starts reuse the `geko-data` volume and only rebuild what changed.

**3. Use it.** The dashboard, a map with the search of the geocoder and a tab for each active module, is at <http://localhost:8080/>. Each module serves its routes under its name: the geocoder under `/geocoder/`, with its API in [its README](https://github.com/candymaps/geko-geocoder#usage). Common to all of them: `/health`, `/metrics`, the licenses and attribution of the served data at `/v1/attribution`, and the API of the host and of every active module, browsable with Swagger UI, at `/docs` (the host's, [openapi.yaml](openapi.yaml), also at `/openapi.yaml`).

To update: `docker pull ghcr.io/candymaps/geko`, remove the container and start it again with the same command; the data stays in the volume.

## Configuration reference (`geko.toml`)

Every section and option of geko itself (the host), with its default value unless a comment says otherwise. geko reads `/etc/geko/geko.toml` (or `GEKO_CONFIG`), and each module its `geko-<module>.toml` beside it: the geocoder's options are in [geko-geocoder](https://github.com/candymaps/geko-geocoder#configuration-geko-geocodertoml).

```toml
# Every module, enabled or not (`true`, `false`, or a table with `enabled` and `config`, the file
# it reads, by default geko-<module>.toml beside this one). A module linked into geko is used as it
# is; otherwise geko downloads it into /var/lib/geko/modules: the newest version of the contract it
# speaks, or the one fixed with `version` (geko plan shows which). `geko serve` uses the version that
# built the bundle it serves, and downloads it if missing.
[modules]
geocoder = true
tiles    = true

# [modules.geocoder]                # instead of `geocoder = true` above
# version  = "1.4.2"                # default: the newest of the contract of this geko
# registry = "ghcr.io/candymaps/geko-geocoder"   # default: ghcr.io/candymaps/geko-<module>

[[scope]]                           # one per country; no default
country = "ES"                      # ISO 3166-1
regions = ["ES-PM"]                 # ISO 3166-2, as the modules know them (the geocoder: from its country packs); empty: the whole country

[data]
refresh            = "if-changed"   # never | if-changed | always
max_age_days       = 30             # days between two checks of every source (geko build --refresh: now)
keep_downloads     = false          # true keeps the downloads after publishing (handy in development)
offline            = false          # never contact the sources; keeps the downloads
accept_licenses    = false          # required to download: review the licenses listed by geko plan
parallel_downloads = 4              # downloads at the same time

[build]
keep = 1                            # published versions kept (2 to be able to roll back)

[serve]
listen                 = "127.0.0.1:8080"   # in Docker, always 0.0.0.0:8080
reload_secs            = 30         # check for a new bundle version this often
cors_origins           = []         # origins a browser page may call the API from ("*": any)
api_keys               = []         # keys a request must carry (Authorization: Bearer, X-Api-Key or api_key=); empty: open
rate_limit_per_s       = 0          # requests per second from one client address (0: none)
rate_limit_burst       = 0          # burst above that rate (0: twice the rate)
bundles_url            = ""         # HTTP address of bundles published on another machine; each reload pulls only what changed
attribution_contact    = ""         # where to ask for the code of the derivative database (ODbL 4.6), in /v1/attribution

# How geko is deployed (docs/OPERATOR_MODE_PLAN.md). The modules never read it.
[deploy]
mode = "single"                     # single: every module in one process, wherever it runs | operator: one deployment per module in Kubernetes
                                    # (not implemented yet: with it, `geko serve|build|up` ask for `--module <name>`, one module per process)

# [deploy.geocoder]                 # only the operator mode reads these
# replicas = 2                      # default 1
# cpu      = "500m"                 # Kubernetes quantities, as the pods ask for them
# memory   = "4Gi"
```

### One module per process

`geko serve --module geocoder` (`--module` is an alias of `--modules`) serves only the routes of that module, and `geko build --module geocoder` builds only its part. The bundle that build publishes keeps what the current one has of the other modules, so each module can build in a job of its own, with its own resources; publications into the same bundles directory wait for each other (`.publish.lock`). A build with no `--module` publishes exactly the modules that are active, as before.

Two probes tell a process apart: `/health` answers from the start (it is alive, also while it builds), and `/ready` answers 503 until a bundle is loaded with the part of every module served there, and 200 from then on. They are the liveness and readiness probes of a pod (docs/OPERATOR_MODE_PLAN.md). Neither needs an API key.

To develop it as it runs in Kubernetes, put `[deploy] mode = "operator"` in the configuration of `make start-dev` (`develop/geko.toml`, or `DEV_CONFIG=…`): the same command then makes a local kind cluster (`geko-dev`) if there is none, loads the image into it, applies the manifests that `geko k8s render` writes from that configuration (a pod per module, a build job per module, one volume for the data, `deploy/`), builds each module one after the other, and serves it all at the same `http://localhost:8080/`. `make stop-dev` deletes what runs and keeps the cluster and its volume; `make stop-dev PURGE=1` deletes the cluster. It needs `docker`, `kind` and `kubectl`, and says so if one is missing. The `operator` workflow runs this on Luxembourg, and `ci/operator.sh` checks that a module that falls does not take the other with it and that each scales on its own.

Without a cluster, `develop/compose-split.yaml` runs it as the cluster would: a build job per module, one after the other, a server per module and an nginx that sends each path to its module (`GEKO_DEV_CONFIG=~/geko-dev.toml docker compose -f develop/compose-split.yaml up --build`, with the same variables as `make start-dev`).

## License

geko is licensed under the [GNU Affero General Public License v3.0](LICENSE) (AGPL-3.0-only). You may use, study, modify and share it freely. If you distribute it, modified or not, you must pass on its source code under the same license; if you modify it and let others use it over a network, you must offer them the source code of your version. Running it, or modifying it for internal use only, requires nothing more.

The data that geko serves keeps the licenses of its sources (ODbL for OpenStreetMap, CDLA for Overture, CC BY for CartoCiudad…), and every response carries their attribution. Contributions are accepted under the same license, from the collaborators of the repository; see [CONTRIBUTING.md](CONTRIBUTING.md).

Read the full terms in [LICENSE](LICENSE).

## Technologies

### Foundation

| Technology | What it is used for |
|---|---|
| **Rust** (2024 edition) | The host: configuration, download, modules, bundles and serving |
| `clap` | Command line: `geko plan`, `fetch`, `build`, `serve`, `up`, `purge` and the commands of each module (`geko geocoder query`…) |
| `serde`, `serde_json`, `toml` | Configuration (`geko.toml`), bundle manifest, JSON responses |
| `thiserror`, `anyhow` | Typed errors in the libraries and errors with context in the binary |
| `tracing`, `tracing-subscriber` | Activity logging (downloads, timing of each phase, reloads) |
| `mimalloc` | Memory allocator; each module declares its own as well |

### Data download

| Technology | What it is used for |
|---|---|
| `ureq` (with `rustls`) | Synchronous HTTP client with pure-Rust TLS: downloads with resume, ETag and retries |
| `sha2`, `hex` | Checksums of downloads and bundle artifacts |
| `ed25519-dalek` | Signatures of the modules and data packages that geko downloads |
| `libloading` | Modules loaded as libraries (`kind = "dylib"`) |

### HTTP service

| Technology | What it is used for |
|---|---|
| `axum` | HTTP server of the host: it mounts the routes of each module under its name (`/geocoder/…`) and serves the shared ones (`/health`, `/metrics`, `/v1/attribution`, `/docs`) |
| `tokio` | Asynchronous server; only `geko serve` uses it |
| `arc-swap` | Atomic swap of the bundle version on hot reload |

### Tooling

| Technology | What it is used for |
|---|---|
| WSL Debian | Build and run environment (Linux, as in production) |
| `cargo`, `clippy`, `rustfmt` | Building, static analysis and formatting |
| `tempfile`, `tiny_http` | Tests: temporary directories and a fake HTTP server for the downloader |
| Conformance suites (own, `geko-module-sdk`) | The contract of a module, which every module runs from its tests, and the downloader |
| `ci/test.sh` (own) | The checks, the same locally and in CI (GitHub Actions) |
