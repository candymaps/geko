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

To write a new one, see [Writing a module]({% link writing-a-module.md %}).

## Download and loading

Without `GEKO_MODULES`, a module is downloaded into
`/var/lib/geko/modules/<module>/<version>/` from `ghcr.io/candymaps/geko-<module>`: the newest one
for the contract of `geko`, or the one set by `[modules.<module>] version`. Modules are signed.
`geko plan` says which version is used and where it comes from.

A module that is disabled is not downloaded, not instantiated, opens no files and registers no
routes.

`geko purge modules` removes the modules that no active module or kept bundle uses.

## Local development

`GEKO_MODULES` points to one directory per module, each with its `module.toml`. The module is then
loaded as a library into the process of `geko`, and a module that is not there is an error, never
a download.

## Options

Every option of a module is documented in its own page and repository, not in `geko.toml` or in the
README of the host.
