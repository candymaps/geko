---
title: Modules
nav_order: 6
has_children: true
---

# Modules

The host knows a module only through the contract of `geko-module-sdk` (the `Module` trait): it
never depends on one. Each module serves its routes under `/<module>/`, reads its own
`geko-<module>.toml` and builds its own part of the bundle.

The modules available:

- [Geocoder]({% link geocoder.md %}): addresses and places into coordinates, and back.
- [Tiles]({% link tiles.md %}): vector tiles of the scope, with styles, fonts and sprites.
- [Router]({% link router.md %}): routes, matrices and the nearest street.

To write a new one, see [Writing a module]({% link writing-a-module.md %}).

## Loading

geko downloads no module. The image of each version carries the modules of its `modules.lock`, signed,
and sets `GEKO_MODULES`, so the version of geko decides which ones it runs. A module that is active and
is neither there nor linked in is an error.

Each module loads, fails and answers on its own: one that rejects its data does not stop the others
(`/health`, `/ready`, and `/ready?module=<module>` for a single one).

A module that is disabled is not instantiated, opens no files and registers no routes.

## Local development

`GEKO_MODULES` points to one directory per module, each with its `module.toml`. The module is then
loaded as a library into the process of `geko`, and a module that is not there is an error.

## Options

Every option of a module is documented in its own page and repository, not in `geko.toml` or in the
README of the host.
