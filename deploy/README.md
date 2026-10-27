# deploy/: the operator mode as manifests

What geko puts in a Kubernetes cluster in the operator mode ([docs/OPERATOR_MODE_PLAN.md](../docs/OPERATOR_MODE_PLAN.md)). Per active module: a `Deployment` and a `Service` (`geko serve --module`, ready when `/ready` says so), a `CronJob` that builds it (`geko build --module`, resources of its own), and, for all of them, one `PersistentVolumeClaim` (bundles and downloads, `ReadWriteMany`) and one `Ingress` that sends `/<module>/` to its service.

There are two ways to have them in a cluster:

- **With the operator** (`geko operator`): install it (`deploy/operator/`) with the config map `geko`, and it makes all of the above from it, keeps it so, deletes what the configuration no longer asks for, starts the build of each module one after the other, and serves the dashboard at `/` with what the cluster runs. This is what `make start-dev` does in a local cluster.
- **Without it** (`deploy/base/`): `geko k8s render` writes the same resources once, to keep in Git and apply with ArgoCD or Flux. For platform teams that do not run third-party operators.

```
deploy/
  geko.toml            what the base is rendered from: the active modules and [deploy]
  base/                resources.yaml, written by `geko k8s render`; never edited
  operator/            resources.yaml, written by `geko k8s install`; never edited
  overlays/local/      a kind cluster with the operator: the image of `make start-dev`, ReadWriteOnce storage
  overlays/ci/         local, with Luxembourg: what the CI applies
  overlays/operator/   an example for a cluster with ReadWriteMany storage and an Ingress controller, with the operator
  overlays/prod/       the same without the operator: replicas, resources, host
```

Change `deploy/geko.toml` (or the code of the renderer) and run `ci/deploy.sh render`; `ci/deploy.sh check` (it is the `deploy` variant of `ci/test.sh`) fails if `base/resources.yaml` or `operator/resources.yaml` is not what geko writes, if an overlay does not build with kustomize, or if what it builds does not pass `kubeconform --strict`. None of it needs a cluster. What the operator makes is what `geko k8s render` writes, so the base checks it.

## The operator

`deploy/operator/` is its `ServiceAccount`, a `Role` in its namespace only (it cannot delete the volume), its `Deployment` and the `Service` of the dashboard. It reads the config map `geko` by that name, so an overlay disables the hash suffix that kustomize adds (`generatorOptions: disableNameSuffixHash: true`); the operator rolls the pods by the digest of the config map itself. The replicas and resources of each module are `[deploy.<module>]` in its `geko.toml`: a `kubectl scale` does not stay, the operator puts the replicas back. Flags that are not in `geko.toml` (`--image-prefix`, `--tag`, `--storage`, `--storage-class`, `--access-mode`, `--host`) are in the args of its `Deployment`: `geko k8s install --help`, then render again, or add them with a patch as `overlays/operator` does.

It builds a module when what its build reads changes (the scope and the sources, not `[serve]`, `[deploy]` or the other modules), by a job named after a digest of that, and never runs the same twice. The cron jobs still run every `[data] max_age_days`, an hour apart, to refresh the data. `[data] keep_downloads = true` goes in the `geko.toml` of an overlay: the builds share `raw/`, and the one that runs second would otherwise download again what the first removed.

The overlays make the config map `geko` (`geko.toml` and, beside it, one `geko-<module>.toml` per module that has options). The images are `ghcr.io/candymaps/geko-<module>` and, for the operator, `ghcr.io/candymaps/geko` (the host with no module); the local overlay points them all to the one image that `make start-dev` builds with both modules, which is loaded into the cluster.

Without the operator, to build now instead of waiting for the schedule: `kubectl create job --from=cronjob/geko-build-geocoder first-geocoder`, and the same for each module, one after the other.
