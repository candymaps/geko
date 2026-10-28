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

- [Modules]({% link modules.md %}): how the host loads a module and what the contract with it is.
- The [README](https://github.com/candymaps/geko#readme) has the installation and usage and the
  configuration reference.
- The specification of the host is in
  [PROJECT.md](https://github.com/candymaps/geko/blob/main/docs/PROJECT.md), and the contract with
  the modules in
  [MODULES_PLAN.md](https://github.com/candymaps/geko/blob/main/docs/MODULES_PLAN.md).
