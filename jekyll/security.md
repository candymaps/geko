---
title: Security and licenses
nav_order: 10
---

# Security and licenses
{: .no_toc }

1. TOC
{:toc}

## Signed code

Downloaded code runs only with a valid signature. A module is downloaded only if it carries a signature
made with one of the keys that geko trusts, and it is for the platform of the host. Data packages (the
country packs of the geocoder) are signed in the same way.

A module can also be linked into the process, or loaded from `GEKO_MODULES` for development. A module
loaded as a library runs in the process of geko, so a hard failure of it (a segfault, running out of
memory) stops that process. To isolate modules from each other, use one process per module: see
[Kubernetes]({% link kubernetes.md %}).

## Access to the API

By default, the API is open. These options of `[serve]` change that:

```toml
[serve]
cors_origins     = []   # origins a browser page may call the API from ("*": any)
api_keys         = []   # keys a request must carry (Authorization: Bearer, X-Api-Key or api_key=); empty: open
rate_limit_per_s = 0    # requests per second from one client address (0: none)
rate_limit_burst = 0    # burst above that rate (0: twice the rate)
```

With keys set, every route asks for one, except:

- `/` (the [dashboard]({% link dashboard.md %}) files);
- `/health`;
- `/v1/attribution`;
- the routes a module declares public (such as its demo page).

`/ready`, the readiness probe of a process, does not need a key either. The routes of the dashboard under
`/admin/api/` are behind the key.

## Licenses of the code

The code is licensed under the
[GNU Affero General Public License v3.0](https://github.com/candymaps/geko/blob/main/LICENSE)
(AGPL-3.0-only). You may use, study, modify and share it freely.

- If you distribute it, modified or not, you must pass on its source code under the same license.
- If you modify it and let others use it over a network, you must offer them the source code of your
  version.
- Running it, or modifying it for internal use only, requires nothing more.

Contributions are accepted under that same license, with no CLA: only the collaborators of the repository
may open pull requests. Without a CLA, the code of others cannot be relicensed or given under commercial
licenses.

## Licenses of the data

geko downloads data from sources with their own conditions. Downloading automatically means accepting
them, so it requires `[data] accept_licenses = true`. The sources of the default configuration:

| Source | License |
|---|---|
| OpenStreetMap | ODbL |
| Overture Maps | CDLA |
| CartoCiudad (Spain) | CC BY |

Every module declares the licenses of what it downloads. `geko plan` lists them before anything is
downloaded, and they go into the manifest of each bundle.

The geocoder serves a derivative database of OpenStreetMap, and the tiles are a Produced Work of it. Both
carry its attribution.

### Attribution

`GET /v1/attribution` (no API key) returns:

- the licenses and attributions of the sources of the bundle;
- whom to ask for the code of the derivative database (`[serve] attribution_contact`, ODbL 4.6);
- what each module adds: the geocoder, the notice of a derivative database and where its method is
  (`/geocoder/v1/attribution/method`).

This is not legal advice: before opening a service to the public, have a lawyer review it.
