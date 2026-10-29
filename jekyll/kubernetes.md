---
title: Kubernetes
nav_order: 7
has_children: true
---

# Kubernetes
{: .no_toc }

geko runs as one process with every module in it (`single`), anywhere: Docker, compose, a server or a
pod. In a cluster it can also run in the **operator mode**: one deployment per module, so a module can
fail, scale and be updated on its own. To get the files for a cluster from a few choices, use the
[wizard]({% link wizard.md %}).

1. TOC
{:toc}

## Why one process per module

When modules are loaded as libraries into one process, which is ideal for a `docker run`, a cluster has
three problems:

- **A serious failure of one module takes the others down.** A panic only fails its request, but a
  segfault, a stack overflow or running out of memory stops the whole process.
- **They do not scale separately.** Tiles (disk, network, many small requests) and the geocoder (RAM,
  CPU per query) cannot have different replicas, requests or limits in the same pod.
- **They do not update separately.** A new version of a module needs a restart, and with it the others
  restart.

## Choosing the mode

The mode is set in the configuration. It is never detected on its own, because without the right
permissions it would fail, and a platform team wants to know what creates what in its cluster.

```toml
[deploy]
mode = "single"    # single (default) | operator
```

- `single`: every module in one process. It works anywhere, and it is the default.
- `operator`: one deployment per module. With it, `geko serve`, `geko build` and `geko up` ask for
  `--module <name>`: one module per process.

Replicas and resources per module go in their own section, which `single` ignores:

```toml
[deploy]
mode = "operator"

[deploy.geocoder]
replicas = 2
cpu      = "500m"     # Kubernetes quantities, as the pods ask for them
memory   = "4Gi"

[deploy.tiles]
replicas = 4
```

Everything else (`[modules]`, `[[scope]]`, `[data]`, the `geko-<module>.toml` files) is the same in both
modes: only `[deploy]` changes between development and production.

The modules do not know in which mode they run. The requests go from the Ingress to the pod of the
module, without going through geko.

## What is put in the cluster: `geko k8s render`

```bash
geko k8s render > resources.yaml
```

It writes to the standard output the manifests for the active modules and the `[deploy]` section. It
needs no module downloaded, and no cluster. Per active module:

- a `Deployment` and a `Service` (`geko serve --module`, ready when `/ready` says so);
- a `CronJob` that builds it (`geko build --module`, with resources of its own).

And for all of them:

- one `PersistentVolumeClaim`, `ReadWriteMany`, for bundles and downloads;
- one `Ingress` that sends `/<module>/` to its service.

The images are `ghcr.io/candymaps/geko-<module>`, one per module: the host with that module linked into
it. Options of `render`: `--image-prefix`, `--tag`, `--storage` (default `20Gi`) and `--storage-class`.
See the [CLI]({% link cli.md %}).

### Probes

- `/health` answers from the start: it is the liveness probe.
- `/ready` answers 503 until the pod has loaded a bundle with the part of its module, and 200 from then
  on: it is the readiness probe. Kubernetes sends no traffic before.

### Storage

Bundles live in a PVC with `ReadWriteMany` access: the build job writes in it, and the pods of every
module mount it to read and reload, even on different nodes. A `ReadWriteOnce` volume does not work in
production, because it mounts on one node at a time. The cluster needs a storage class that offers
`ReadWriteMany` (NFS, CephFS, Azure Files, EFS, Filestore…).

## The `deploy/` folder

The repository keeps the manifests as a kustomization, with a single source:

```
deploy/
  geko.toml            what the base is rendered from: the active modules and [deploy]
  base/                resources.yaml, written by `geko k8s render`; never edited
  overlays/local/      a kind cluster: the image of `make start-dev`, the Balearic Islands, ReadWriteOnce storage
  overlays/ci/         local, with Luxembourg: what the CI applies
  overlays/prod/       an example for a cluster with ReadWriteMany storage: replicas, resources, host
```

The overlays make the config map `geko` (`geko.toml` and, beside it, one `geko-<module>.toml` per module
that has options). Apply one with `kubectl apply -k deploy/overlays/<overlay>`.

### Builds

The builds are cron jobs that share the downloads in `raw/`, so they run an hour apart. Put
`[data] keep_downloads = true` in the `geko.toml` of an overlay: the build that runs second would
otherwise download again what the first removed.

To build now instead of waiting for the schedule, one module after the other:

```bash
kubectl create job --from=cronjob/geko-build-geocoder first-geocoder
```

When a build publishes a new bundle, the pods detect it at their next reload (`[serve] reload_secs`),
download only what changed and switch atomically, with no restart.

### Checks without a cluster

Change `deploy/geko.toml` (or the code of the renderer) and run `ci/deploy.sh render`.
`ci/deploy.sh check` (the `deploy` variant of `ci/test.sh`) fails if:

- `base/resources.yaml` is not what the renderer writes;
- an overlay does not build with kustomize;
- what it builds does not pass `kubeconform --strict`.

## Developing it locally

Put `[deploy] mode = "operator"` in the configuration of `make start-dev` (`develop/geko.toml`, or
`DEV_CONFIG=…`). The same command then:

1. makes a local kind cluster (`geko-dev`) if there is none;
2. loads the image into it;
3. applies the manifests that `geko k8s render` writes from that configuration;
4. builds each module one after the other;
5. serves it all at the same `http://localhost:8080/`.

`make stop-dev` deletes what runs and keeps the cluster and its volume. `make stop-dev PURGE=1` deletes
the cluster. It needs `docker`, `kind` and `kubectl`, and says so if one is missing.

Without a cluster, `develop/compose-split.yaml` runs it as the cluster would: a build job per module,
one after the other, a server per module and an nginx that sends each path to its module:

```bash
GEKO_DEV_CONFIG=~/geko-dev.toml docker compose -f develop/compose-split.yaml up --build
```

The `operator` workflow of the CI runs this on Luxembourg, and `ci/operator.sh` checks that a module that
falls does not take the other with it and that each scales on its own. It needs the secret
`GEKO_MODULES_TOKEN`, and it does not run without it.

## Limits

- A local cluster does not reproduce the `ReadWriteMany` volume of production, real latency and memory
  limits, the Ingress of the platform or a hardened RBAC.
- The `Ingress` is the default route. Gateway API is not decided yet.
- There is no controller that reconciles the cluster when `geko.toml` changes: changing it, or raising a
  module version, means rendering and applying the manifests again.
