---
title: Tiles
parent: Modules
nav_order: 2
---

# Tiles
{: .no_toc }

The vector tiles module of geko: a map of your scope, served by geko alone. It lives in
[candymaps/geko-tiles](https://github.com/candymaps/geko-tiles). It is published, and enabled by default
in geko.

It downloads what the scope needs, generates the tiles with [Planetiler](https://github.com/onthegomap/planetiler),
leaves its part of the bundle (all of Spain in 14 minutes) and serves it, with styles, fonts and
sprites.

1. TOC
{:toc}

## What it serves

All under `/tiles/`:

| Route | What it does |
|---|---|
| `/tiles/{z}/{x}/{y}` | Vector tiles, MVT by default and MLT as an option, up to zoom 14, with the [Protomaps Basemap](https://github.com/protomaps/basemaps) schema. A tile that does not exist answers 204, which clients take as empty. |
| `/tiles/tilejson.json` | A TileJSON with the attribution, the layers, the bounds of the scope and versioned tile URLs (`?v=<bundle>`). Those tiles are cached forever by clients and CDNs, and a new bundle changes the URLs. |
| `/tiles/styles/{flavor}/{lang}.json`, `/tiles/styles.json` | The styles of Protomaps, in 5 flavors (`light`, `dark`, `white`, `grayscale`, `black`) and 41 languages. `/tiles/styles.json` lists them. |
| `/tiles/fonts/…`, `/tiles/sprites/…` | The fonts and sprites of the styles. |
| `/tiles/status.json`, `/tiles/stats.json` | The bundle loaded, its tiles, layers, styles and sources and the options in effect; and, since the module started, the requests, errors and time of every route, the tiles of each zoom level, the last hour minute by minute and the styles asked for. They feed the dashboard of geko. |

The styles are data: any other MapLibre style can paint the tiles, pointing to `tilejson.json`, and the
server never knows which one.

## Enable it

In `geko.toml` (it is on by default):

```toml
[modules]
tiles = true
```

Its own options go in `geko-tiles.toml`, beside `geko.toml`. Every one is optional:

```toml
format = "mvt"        # or "mlt": about 10 % smaller, read only by MapLibre GL JS 5.12 and later
url = "/tiles"        # where clients fetch the tiles, fonts and sprites, as tilejson.json and the styles write it: absolute
                      # (https://maps.example.com/tiles) for clients that load a style or a TileJSON by its URL (MapLibre
                      # needs the tiles and the sprite absolute), behind a CDN or behind a proxy with a prefix

[osm]
mirror = "auto"       # Geofabrik, or the URL of a mirror with its layout (the extracts are shared with the geocoder)

[planetiler]
memory = "4g"         # heap of the Java process (-Xmx)
threads = 0           # 0: one per core
storage = "mmap"      # or "ram": where Planetiler keeps its temporary data
qrank = true          # download QRank to order places and POIs; the build goes on without it
```

Planetiler's memory, threads and storage change how fast the build runs and how much memory it takes,
never the tiles.

## What it downloads

Downloading requires `[data] accept_licenses = true` in `geko.toml`, as for every module.

- OSM extracts, the Natural Earth layers it uses, and the water and land polygons, cut to the scope on
  the machine.
- The official `planetiler.jar` and a Java runtime (Temurin), from their origin and checked against
  fixed checksums. The JVM runs only during the build, in a process of its own: it never enters geko.
- The fonts (Noto Sans, OFL) and sprites (icons of Tangram, MIT) of the styles, from
  [protomaps/basemaps-assets](https://github.com/protomaps/basemaps-assets) at a fixed commit.

The machine that builds needs `libfontconfig1`: the Java runtime reads the fonts of the profile with it.

## Performance

A tile is one lookup in an index pinned in RAM and a copy of its bytes, already compressed: about 2 µs,
and millions of tiles per second. All of Spain takes 1.17 GB on disk and 9 MB in RAM.

## License

The tiles are a Produced Work of OpenStreetMap (ODbL) and carry its attribution. Planetiler and the Java
runtime are not redistributed: each installation downloads them from their origin, with their own
licenses.
