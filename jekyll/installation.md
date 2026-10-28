---
title: Installation and usage
nav_order: 3
---

# Installation and usage
{: .no_toc }

All you need is Docker.

1. TOC
{:toc}

## 1. Write a `geko.toml`

It declares the modules and the countries to serve:

```toml
[modules]
geocoder = true          # geocoding
tiles    = true          # vector tiles; more modules (routing…) are on the way

[[scope]]
country = "ES"            # ISO 3166-1; one [[scope]] per country (LU…)
# regions = ["ES-PM"]     # only part of it: ISO 3166-2, from the regions in the country pack; without regions, the whole country

[data]
accept_licenses = true    # you accept the licenses of the sources: ODbL (OSM), CDLA (Overture), CC BY (CartoCiudad)
```

Every section and option of geko itself is in the [configuration reference]({% link configuration.md %}).
Each module has its own file beside it, `geko-<module>.toml`, documented in its own page:
[geocoder]({% link geocoder.md %}) and [tiles]({% link tiles.md %}). Without it, a module takes its
defaults.

The image carries no configuration: without a mounted `geko.toml`, it stops right away asking for one.
Inside the container, geko always listens on `0.0.0.0:8080`, whatever `[serve] listen` says.

## 2. Start it

```bash
docker run -d --name geko -p 8080:8080 -v geko-data:/var/lib/geko \
  -v "$PWD/geko.toml:/etc/geko/geko.toml:ro" ghcr.io/candymaps/geko
```

To change the options of a module, mount its file too, beside it:
`-v "$PWD/geko-geocoder.toml:/etc/geko/geko-geocoder.toml:ro"`.

The first time, geko downloads the data for those countries, builds the indexes and serves them. It
answers 503 until then: about 2 minutes for Luxembourg, around an hour for all of Spain (about 8 GB of
disk while building and 5 GB afterwards, and 3 GB of memory).

Follow it with `docker logs -f geko`. While it builds, there is one line every 2 seconds with how far
it has gone. With `GEKO_VERBOSE=1` (`-e GEKO_VERBOSE=1` in Docker, or `geko --verbose`) every step is
logged instead. Later starts reuse the `geko-data` volume and only rebuild what changed.

## 3. Use it

The demo page, a map with the search of the geocoder, is at <http://localhost:8080/>.

Each module serves its routes under its name: the geocoder under `/geocoder/`, tiles under `/tiles/`.
Common to all of them:

| Route | What it is |
|---|---|
| `/health` | Liveness: answers from the start, also while building. |
| `/metrics` | Metrics. |
| `/v1/attribution` | Licenses and attribution of the served data. |
| `/docs` | The API of the host and of every active module, browsable with Swagger UI. |
| `/openapi.yaml` | The API of the host. |

## Update

```bash
docker pull ghcr.io/candymaps/geko
```

Then remove the container and start it again with the same command. The data stays in the volume.
