---
title: Geocoder
parent: Modules
nav_order: 1
---

# Geocoder
{: .no_toc }

The geocoding module of geko: addresses and places into coordinates, and back. It lives in
[candymaps/geko-geocoder](https://github.com/candymaps/geko-geocoder). It is published, and enabled by
default in geko.

It downloads its sources and the country packs of the scope, merges them into one entity per street,
address or place, builds an index per country and serves it under `/geocoder/`.

1. TOC
{:toc}

## What it serves

| Route | What it does |
|---|---|
| `/geocoder/v1/search`, `/geocoder/v1/search/structured` | Addresses, streets, places and POIs from free text, with typo tolerance, decimal, DMS and UTM coordinates. |
| `/geocoder/v1/autocomplete` | Results as the user types, biased by a focus point. |
| `/geocoder/v1/reverse` | The address, street, POI or place nearest to a coordinate. |
| `/geocoder/v1/status`, `/geocoder/v1/stats` | The bundle and countries loaded, the sources and the options in effect; and, since the module started, the requests, errors and latency of every route, the last hour minute by minute, the searches that found nothing or that the time budget cut short, and the cache. They feed the dashboard of geko. |
| `/geocoder/v1/place/{id}`, `/geocoder/v1/batch` (POST) | A place by id, and batches. |
| `/geocoder/` | A test page with a map. |
| `/geocoder/openapi.yaml` | The full API. Also browsable with Swagger UI at `/docs`. |
| `/geocoder/v1/attribution/method` | The method of the derivative database (ODbL 4.6). The licenses of the sources are in the host's `/v1/attribution`. |

Examples, with the host on port 8080:

```
http://localhost:8080/geocoder/v1/search?text=gran+via+1,+madrid
http://localhost:8080/geocoder/v1/autocomplete?text=gran+v
http://localhost:8080/geocoder/v1/reverse?lat=40.4168&lon=-3.7038
```

## Enable it

In `geko.toml` (it is on by default):

```toml
[modules]
geocoder = true
```

The host decides which countries and regions to serve, whether downloads are allowed and where it
listens. Everything specific to geocoding goes in the module's own file, beside it:
`/etc/geko/geko-geocoder.toml`, or the file named by `[modules.geocoder] config`.

## Configuration: `geko-geocoder.toml`

Without the file, every option takes its default value.

```toml
# Sources are used by default when they cover some country of the scope: their sections only
# change options, or disable a source with `enabled = false`. OSM cannot be disabled.
[sources.osm]
mirror = "auto"                     # or the URL of a mirror with Geofabrik's layout

[sources.overture]                  # Overture Maps Places (POIs), for every country
enabled        = true
release        = "latest"           # or a fixed release, such as "2026-09-23.1"
min_confidence = 0.8                # places Overture is less sure exist are left out (0 to 1)

[sources.cartociudad]               # declared by the pack of Spain; any key of its declaration can be overridden
enabled = true

[packs]
registry = "ghcr.io/candymaps/geko-geocoder/countries"   # where the country packs are downloaded from

[serve]
countries              = []         # the countries this node serves; empty: every country of the bundle
search_budget_ms       = 15         # time budget of a search (0: none); the likeliest reading always completes
autocomplete_budget_ms = 5          # and of an autocomplete request
cache_entries          = 0          # answers kept for repeated queries, about 2.5 KB each (0: none)
slow_queries           = 0          # slowest queries of the last hour that `/geocoder/v1/stats` lists, without digits or links, in memory only (0: none)
```

Every section and option is in
[develop/geko-geocoder.toml](https://github.com/candymaps/geko-geocoder/blob/main/develop/geko-geocoder.toml),
with its default value and what it does: use it as the reference.

- An unknown option is an error that names the file and the key.
- An option that used to be in `geko.toml` and is now here (`[sources.*]`, `[packs]`,
  `[serve] search_budget_ms`…) stops geko at startup, saying to move it here.
- Options that change what is built (those of the sources) are part of the stamps of the build stages:
  changing one rebuilds only what it affects.

In Docker, mount it beside `geko.toml`:

```bash
docker run -d --name geko -p 8080:8080 -v geko-data:/var/lib/geko \
  -v "$PWD/geko.toml:/etc/geko/geko.toml:ro" \
  -v "$PWD/geko-geocoder.toml:/etc/geko/geko-geocoder.toml:ro" ghcr.io/candymaps/geko
```

## Data sources

By default, the data comes from OpenStreetMap and Overture Maps Places (POIs), for any country. Each
country can also contribute its own sources in its pack (`countries/<country>/`), and the geocoder
merges them all into a single source of truth, with no duplicates. Spain, for example, adds
CartoCiudad.

Downloading requires `accept_licenses = true` in the `[data]` section of `geko.toml`, after reviewing
the licenses listed by `geko plan`.

## Debugging commands

They run on the last bundle built:

| Command | What it does |
|---|---|
| `geko geocoder parse "<query>"` | The readings of a query. |
| `geko geocoder query "<query>" [--autocomplete] [--focus lat,lon] [--debug]` | A search, with the time of each reading, without the time budget. |
| `geko geocoder reverse <lat> <lon>` | A reverse lookup. |

`geko geocoder` lists them.

## Performance

With all of Spain, p99 is 4.0 ms for search and 5.6 ms for autocomplete, within a time budget per query.
It uses an index of words per country, with the hot part pinned in RAM and the cold part mapped.
