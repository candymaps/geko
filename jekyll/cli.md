---
title: CLI
nav_order: 5
---

# Command line
{: .no_toc }

```
geko [--modules <list>] [--verbose] <command>
```

1. TOC
{:toc}

## Global options

| Option | What it does |
|---|---|
| `--modules <a,b>` (alias `--module`) | Comma-separated active modules. It replaces the `[modules]` section. One module (`--module geocoder`) is how a pod of the operator mode runs: it builds and serves only its own. |
| `--verbose` | Logs every step of a build, instead of its progress every 2 seconds. Same as `GEKO_VERBOSE=1`. |

## Environment variables

| Variable | What it does |
|---|---|
| `GEKO_CONFIG` | Path of `geko.toml` (default `/etc/geko/geko.toml`). |
| `GEKO_DATA` | Directory of the data (default `/var/lib/geko`). |
| `GEKO_MODULES` | One directory per module, with its `module.toml`. Each is loaded as a library into the process. A module that is not there is an error, never a download. |
| `GEKO_VERBOSE` | `1`: as `--verbose`. |

## Commands

### `geko plan`

Shows which sources and data a build would use, and which version of each module is used and where it
comes from. It downloads only small catalog metadata. It lists the licenses to review before setting
`accept_licenses = true`.

### `geko fetch [--refresh]`

Downloads the data needed by the active modules and the enabled sources.

- `--refresh`: checks the sources for changes now, even if `[data] max_age_days` has not passed.

### `geko build [--force] [--purge] [--refresh]`

Downloads what is missing or changed, then rebuilds what changed and publishes a bundle.

- `--force`: rebuilds every stage, even those whose code and inputs did not change.
- `--purge`: first removes the downloads and work files of countries, regions and sources that the
  configuration no longer asks for.
- `--refresh`: as in `fetch`.

The build is incremental: each stage leaves a stamp and is skipped if neither its code nor its inputs
change.

### `geko serve [--listen <addr>]`

Serves the active modules. It never downloads sources.

### `geko up [--force] [--purge] [--refresh] [--listen <addr>]`

Serves, and builds in the background what the configuration asks for: now, and again when a module asks
for it (the geocoder: every `[data] max_age_days`). The bundle being served stays until the new one is
published. This is what the Docker image runs.

### `geko purge [data | packages [--all] | modules]`

Removes what the configuration no longer asks for. Published bundles stay: `[build] keep` retires them.
Without a subcommand, it removes all of the following.

| Subcommand | Removes |
|---|---|
| `data` | The downloads and work files of countries, regions and sources that the configuration no longer asks for (what `build --purge` removes, without building). |
| `packages [--all]` | The data packages the modules downloaded (the country packs of the geocoder) for the countries outside the scope. With `--all`, all of them: they are downloaded again when a build needs them. |
| `modules` | The downloaded versions of the modules that neither the configuration nor any kept bundle needs. |

### `geko k8s render [options]`

Writes to the standard output the manifests for the active modules and the `[deploy]` section: a
deployment and a service per module, a cron job that builds it, the volume they share and the Ingress.
It needs no module downloaded, nor a cluster.

| Option | Default |
|---|---|
| `--image-prefix` | `ghcr.io/candymaps/geko-` (the image of a module is this prefix followed by its name) |
| `--tag` | `latest` |
| `--storage` | `20Gi` (size of the volume of bundles and downloads, which every module shares) |
| `--storage-class` | the cluster default |

### `geko <module> <command>`

A command of an active module. `geko <module>` lists them. For example, the geocoder has
`geko geocoder query "Calle Mayor 5"`: see the [geocoder page]({% link geocoder.md %}).
