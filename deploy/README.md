# deploy/: the operator mode as manifests

What geko puts in a Kubernetes cluster in the operator mode ([docs/OPERATOR_MODE_PLAN.md](../docs/OPERATOR_MODE_PLAN.md)), as a kustomization. Per active module: a `Deployment` and a `Service` (`geko serve --module`, ready when `/ready` says so), a `CronJob` that builds it (`geko build --module`, resources of its own), and, for all of them, one `PersistentVolumeClaim` (bundles and downloads, `ReadWriteMany`) and one `Ingress` that sends `/<module>/` to its service.

```
deploy/
  geko.toml            what the base is rendered from: the active modules and [deploy]
  base/                resources.yaml, written by `geko k8s render`; never edited
  overlays/local/      a kind cluster: the image of `make start-dev`, the Balearic Islands, ReadWriteOnce storage
  overlays/ci/         local, with Luxembourg: what the CI applies
  overlays/prod/       an example for a cluster with ReadWriteMany storage: replicas, resources, host
```

Change `deploy/geko.toml` (or the code of the renderer) and run `ci/deploy.sh render`; `ci/deploy.sh check` (it is the `deploy` variant of `ci/test.sh`) fails if `base/resources.yaml` is not what the renderer writes, if an overlay does not build with kustomize, or if what it builds does not pass `kubeconform --strict`. None of it needs a cluster.

The builds are cron jobs that share the downloads in `raw/`, so they run an hour apart, and `[data] keep_downloads = true` goes in the `geko.toml` of an overlay: the build that runs second would otherwise download again what the first removed. To build now instead of waiting for the schedule: `kubectl create job --from=cronjob/geko-build-geocoder first-geocoder`, and the same for each module, one after the other.

The overlays make the config map `geko` (`geko.toml` and, beside it, one `geko-<module>.toml` per module that has options). The images are `ghcr.io/candymaps/geko-<module>`; the local overlay points them to the one image that `make start-dev` builds with both modules, which is loaded into the cluster.
