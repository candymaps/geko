---
title: Dashboard
nav_order: 8
---

# Dashboard
{: .no_toc }

geko serves a small web application at `/`: a map with the search of the geocoder, and a tab for every
active module. It is read only: it changes nothing.

1. TOC
{:toc}

## How it is served

- It is a small Preact application embedded in the `geko` binary. There is no Node in the image or in
  production, and it takes no appreciable resources on the server or in the browser.
- Its files are served at `/`, without an API key. Navigation uses the hash of the URL (`#/tiles`), so a
  direct link to a tab needs no route on the server.
- It reads `GET /admin/api/modules` (the active modules, their live version, and whether they are
  unavailable), which is behind the API key like any route that is not public, and the routes of the
  modules.
- Light and dark themes follow the system. `Ctrl` + `.` folds the sidebar.

## The map

The map is MapLibre. With the `tiles` module active, it draws its tiles (light or dark style, and the
language of the browser); otherwise it draws OpenStreetMap. It is not limited to the scope: it moves and
zooms out freely.

The search of the geocoder (autocomplete, search, reverse geocoding, focus pin) appears only if the
`geocoder` module is active.

## The tab of a module

Each active module has its own tab, which reads the status and the statistics the module serves.

| Module | Sections |
|---|---|
| [Geocoder]({% link geocoder.md %}) | Overview, Sources, Performance, Query debugger, Configuration |
| [Tiles]({% link tiles.md %}) | Overview, Sources, Performance, Styles, Configuration |

### Performance

The performance section of both modules shows, since the module started and for the last hour minute by
minute:

| Card | Geocoder | Tiles |
|---|---|---|
| Requests per second | line chart | line chart |
| p99 latency | line chart, in ms | p99 time in the module, in µs |
| Per route | latency table: requests, req/s, p50, p95 and p99, errors | time in the module by route, with the same columns |
| Health | budget exhausted, searches without results, cache hits, duplicates merged | tiles served, bytes sent, empty (204), errors; by zoom level; responses |
| Quality | confidence of the first result and its layer; slowest queries (with `serve.slow_queries`) | most requested styles |

**Range.** The selector at the top chooses how far back the charts look: the last 60 seconds, 5, 15 or
30 minutes, or the last hour. A range of minutes reads the minute-by-minute window; the view asks for new
data as often as a point of its range can change.

**Hovering a chart** shows a guide, a dot on each line and a tooltip with the value and how long ago it
was.

### Pods

With several pods, the dashboard asks every one and shows them together:

- A **Pods** filter above the panels has one button per pod, with its own color. It is not drawn with a
  single pod.
- Each pod has a line in its color, and a dashed line with the total, when it means something.
- Click a pod to **focus** on it: the panels show only that pod, and its color becomes the accent of the
  charts. Click it again to show all.
- The percentiles of several pods are the worst of them, because a percentile of the whole cannot be
  computed from those of its parts.

A geko with no operator has no pods route, and then its one process is the only pod (`geko`).
