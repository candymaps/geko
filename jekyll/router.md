---
title: Router
parent: Modules
nav_order: 3
---

# Router
{: .no_toc }

The routing module of geko: routes on the roads of your scope, served by geko alone. It lives in
[candymaps/geko-router](https://github.com/candymaps/geko-router). It is off by default in geko, and
for now it has the profile of cars.

It downloads the OSM extracts of the scope, builds a graph with turn costs and restrictions and leaves
its part of the bundle (all of Spain in about 4 minutes, 1.1 GB on disk).

1. TOC
{:toc}

## What it serves

All under `/router/`:

| Route | What it does |
|---|---|
| `/router/v1/route` | The fastest way between two points, with its time, length and line, its steps (`steps=true`) and up to three alternatives (`alternatives=n`). |
| `/router/v1/table` | Times and distances from many points to many, up to 1,000,000 cells and 2,000 points a side, by `GET` or `POST`. |
| `/router/v1/nearest` | The nearest street to a point. |
| `/router/v1/isochrone` | What can be reached from a point in up to four times, as GeoJSON polygons. |
| `/router/v1/match` | The roads a trace of up to 2,000 points went along, by `GET` or `POST`. |
| `/router/v1/trip` | A good order to visit up to 100 points, and its routes, by `GET` or `POST`. It is a heuristic, not always the best order. |
| `/router/v1/status` | The size of the graph, where it is and the state of traffic. |
| `/router/openapi.yaml` | Its API, also listed in `/docs`. |
| `/router/` | A demo: click on the map for the start and the end. |

## Enable it

In `geko.toml`:

```toml
[modules]
router = true
```

A country needs a pack, as in the geocoder: there are packs for Spain and Luxembourg. Downloading
requires `[data] accept_licenses = true`.

Its own options go in `geko-router.toml`, beside `geko.toml`. Every one is optional, and they are
documented in the
[README of the module](https://github.com/candymaps/geko-router#installation-and-usage).

## Traffic

Off by default. With `[traffic] enabled = true`, the live sensors of Madrid change the weights while
geko serves, with no restart, the build learns the usual speed of each hour of the week, and every
response says how much of the route had data (`traffic.coverage`). `traffic=none|historical|live` and
`depart_at` choose the weights of a request. The incidents of the DGT, which close roads, stay off
until their license is clear. The speeds are estimates: they were measured on fast roads, and the
delay of city junctions is not in them.

## Profiles

`[profiles] default` in `geko-router.toml` builds the graph for `car`, `truck`, `bike` or `foot`. There
is one profile for each bundle: to serve several, run an instance of geko for each. Only cars and
trucks read traffic.
