---
title: Data and bundles
nav_order: 9
---

# Data and bundles
{: .no_toc }

Everything is automatic: only what the active modules ask for is downloaded and built. An active module
tells geko what it needs for the scope; a module that is disabled downloads nothing.

1. TOC
{:toc}

## Downloads

Modules that are not linked into geko or listed in `GEKO_MODULES` are downloaded first, from
`ghcr.io/candymaps/geko-<module>` (`[modules.<module>] registry` changes where from), into
`/var/lib/geko/modules/<module>/<version>/`. It is the newest version of the contract of this geko, or
the one fixed with `version`, and only if it carries a signature that geko trusts and is for the platform
of the host. See [Security and licenses]({% link security.md %}).

Then each module downloads the data it needs for the scope. The downloader handles:

- **Safe writes:** a download is written to a `.part` file and renamed when it ends, so a half download
  never looks complete.
- **Resuming** with HTTP Range.
- **Retries** with growing waits and a limit of connections (`[data] parallel_downloads` in total).
- **Conditional requests** with ETag and `If-Modified-Since`, according to `[data] refresh`.
- **Checksums** verified when the source publishes them.
- **Offline:** with `[data] offline = true`, no request is made; if something is missing, geko fails with
  the list of what is missing.
- **OCI artifacts** (modules, data packages) from registries such as ghcr.io.

Downloading requires `[data] accept_licenses = true`, and `geko plan` lists every license first.

### The cache

Downloads are kept in `raw/<source>/`, with a `<file>.raw.json` beside each one: URL, ETag,
`Last-Modified`, sha256, size and date. Downloads in progress use `<file>.part` and
`<file>.part.meta.json`, so that they can resume.

**When sources are checked** (`[data] max_age_days`, 30 by default): all sources at once, with a local
mark of the last check (`raw/.checked`).

- While the age has not passed, nothing is checked: each download is kept and only what is missing is
  downloaded.
- When it passes, or with `geko build --refresh`, each source asks whether it changed and only what
  changed is downloaded.
- `refresh = "always"` downloads everything, and `"never"` never checks.

**Deleted downloads:** with `keep_downloads = false` (the default), after publishing a bundle each module
deletes what it declares deletable and keeps its records. With `offline = true`, nothing is deleted.

## Incremental builds

Each stage of a module is identified by the hash of its inputs and of the version of its code. If it does
not change, the stage is not repeated.

- **Stamp:** each stage writes `<dir>.stamp.json` beside its output, with the key (code and inputs) and
  the content of the output. The stamp is removed before the stage starts and written only if it ends
  well: an interrupted stage is never taken for current.
- **Determinism:** the generation is deterministic, so a stage that repeats and writes the same bytes does
  not force the following ones to repeat.
- **Publishing:** no new version is published if the current one has the same parts, modules and licenses.
- **Forcing:** `geko build --force` repeats every stage.

## Bundles

A build publishes a **bundle**: an immutable, versioned folder with the part of every module.

```
bundles/
  v2026-10-15T030000/
    manifest.json             # version, modules (version, contract), licenses, hash of each artifact
    <what each module made>   # e.g. segments/<country>/ of the geocoder
  current -> v2026-10-15T030000
  current.version             # the version of current, for whoever reads it over HTTP
```

- **Generation:** `geko build` publishes a new version, hard-linking what did not change from the previous
  one. It never modifies an existing one, and a bundle in use is never modified.
- **Retention:** the last `[build] keep` versions are kept (2 to be able to roll back).
- **Serving:** `geko serve` checks for a new version every `[serve] reload_secs`, loads each module and
  switches atomically. Queries in progress end on the previous version, and nothing restarts.
- **Another machine:** with `[serve] bundles_url`, the folder of bundles of another machine, served by any
  web server or object storage, is the source. Each reload reads `current.version` and downloads **only
  the artifacts whose hash changed**. The downloads go through `.downloads/`, and the new version becomes
  `current` only when complete, with checksums verified.

Building and serving can be on different machines. The builds of different modules publish into the same
bundles directory and wait for each other (`.publish.lock`).

### Hot and cold

The files of a bundle under a `hot/` directory are pinned in RAM when loaded. The rest is mapped without
pinning, and the operating system caches what is queried. Each module decides what goes in each part.

## Scaling

First, replicas behind a load balancer. Any distribution among nodes is up to each module (the geocoder:
`[serve] countries` in its own file). For one module per process in a cluster, see
[Kubernetes]({% link kubernetes.md %}).
