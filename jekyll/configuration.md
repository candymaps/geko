---
title: Configuration
nav_order: 3
---

# Configuration (`geko.toml`)
{: .no_toc }

Every section and option of geko itself (the host), with its default value unless a comment says
otherwise. geko reads `/etc/geko/geko.toml` (or the file named by `GEKO_CONFIG`), and each module reads
its own `geko-<module>.toml` beside it. The options of a module are documented in its page:
[geocoder]({% link geocoder.md %}), [tiles]({% link tiles.md %}).

1. TOC
{:toc}

## Reference

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
regions = ["ES-PM"]                 # ISO 3166-2, as the modules know them; empty: the whole country

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

# How geko is deployed. The modules never read it.
[deploy]
mode = "single"                     # single: every module in one process, wherever it runs | operator: one deployment per module in Kubernetes
                                    # (with it, `geko serve|build|up` ask for `--module <name>`, one module per process)

# [deploy.geocoder]                 # only the operator mode reads these
# replicas = 2                      # default 1
# cpu      = "500m"                 # Kubernetes quantities, as the pods ask for them
# memory   = "4Gi"
```

## Where the files and paths are

| What | Where |
|---|---|
| The binary | `/usr/local/bin/geko` |
| The configuration of geko | `/etc/geko/geko.toml`, or `GEKO_CONFIG` |
| The configuration of a module | `geko-<module>.toml`, beside `geko.toml` |
| The data (downloads, bundles, modules) | `/var/lib/geko`, or `GEKO_DATA` |
| The modules downloaded | `/var/lib/geko/modules/<module>/<version>/` |

There is no `--config` option and no `[data] dir`: only `GEKO_CONFIG` and `GEKO_DATA` change those
paths.

## Licenses

Downloading requires `[data] accept_licenses = true`, after reviewing the licenses that `geko plan`
lists.

## One module per process

`geko serve --module geocoder` (`--module` is an alias of `--modules`) serves only the routes of that
module, and `geko build --module geocoder` builds only its part. The bundle that build publishes keeps
what the current one has of the other modules, so each module can build in a job of its own, with its
own resources. Publications into the same bundles directory wait for each other (`.publish.lock`). A
build with no `--module` publishes exactly the modules that are active.

Two probes tell a process apart:

- `/health` answers from the start: the process is alive, also while it builds.
- `/ready` answers 503 until a bundle is loaded with the part of every module served there, and 200
  from then on.

They are the liveness and readiness probes of a pod. Neither needs an API key.
