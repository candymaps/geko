---
title: Home
layout: home
nav_order: 1
permalink: /
---

# geko

geko is a modular, multi-country geospatial engine. This repository holds the host: the `geko`
binary (configuration, scope, download and loading of the modules, publication and reload of
bundles, common HTTP) and the SDK its modules are written against.

Each module lives in its own repository:

- the geocoder, in [candymaps/geko-geocoder](https://github.com/candymaps/geko-geocoder), with the
  country packs;
- tiles, in [candymaps/geko-tiles](https://github.com/candymaps/geko-tiles).

## Where to go

- [Architecture]({% link architecture.md %}): the host, the modules and how they fit.
- [Installation and usage]({% link installation.md %}): run geko with Docker in two commands.
- [Configuration]({% link configuration.md %}): every option of `geko.toml`.
- [CLI]({% link cli.md %}): the commands of the `geko` binary.
- [Modules]({% link modules.md %}): how the host loads a module, with a page for the
  [geocoder]({% link geocoder.md %}) and for [tiles]({% link tiles.md %}), and how to
  [write one]({% link writing-a-module.md %}).
- [Kubernetes]({% link kubernetes.md %}): one deployment per module.
- [Dashboard]({% link dashboard.md %}): the map, the status and the performance of every module.
- [Data and bundles]({% link data-and-bundles.md %}): downloads, incremental builds and bundles.
- [Security and licenses]({% link security.md %}): signed code, API keys, and the licenses of code and data.

The specification of the host is in
[PROJECT.md](https://github.com/candymaps/geko/blob/main/docs/PROJECT.md), and the contract with
the modules in
[MODULES_PLAN.md](https://github.com/candymaps/geko/blob/main/docs/MODULES_PLAN.md).
