---
title: Wizard
parent: Kubernetes
nav_order: 1
---

# Wizard
{: .no_toc }

The files to put geko in a cluster with the [operator mode]({% link kubernetes.md %}), from a few choices. Nothing is
sent anywhere: the zip is written in your browser.

<button type="button" class="btn btn-primary wizard-btn" id="wizard-open">Open the wizard</button>

## What you get

```
kustomization.yaml          what `kubectl apply -k` reads
namespace.yaml
operator.yaml               the operator, as `geko k8s install` writes it
README.md
geko-provisioning/
  geko.toml                 modules, scope, [data] and the replicas and resources of each module
  geko-geocoder.toml        the options of the module, if it has any
```

Unzip it, keep it in Git and apply it:

```sh
kubectl apply -k .
kubectl -n geko get pods
```

The operator reads `geko-provisioning/` as the config map `geko`. To change the cluster, edit those files and apply again.

## What it does not do

- **Pin what it cannot know.** The images are `ghcr.io/candymaps/geko:<version>` for the operator and
  `ghcr.io/candymaps/geko-<module>:<version>` for each module. Pin a version: `latest` moves on every release, and a
  module is not updated when the operator is.
- **Pull from a private registry.** The manifests carry no `imagePullSecret`: the cluster needs the credentials on its own.
- **Install an Ingress controller or a storage class.** The volume that the modules share is `ReadWriteMany` unless you
  choose a cluster of one node; its class has to offer it.
- **Set the options of each module.** They are in the repository of each one, and the wizard leaves them for you to edit
  in `geko-provisioning/`.

To write `operator.yaml` yourself, run `geko k8s install` (see [CLI]({% link cli.md %})).

<dialog id="wizard-dialog" class="wizard" aria-labelledby="wizard-title">
  <form id="wizard-form" novalidate>
    <header>
      <h2 id="wizard-title">geko in Kubernetes</h2>
      <button type="button" class="wizard-close" id="wizard-close" aria-label="Close">&times;</button>
    </header>

    <fieldset>
      <legend>Cluster</legend>
      <label>Namespace <input name="namespace" value="geko" autocomplete="off"></label>
      <label>Host of the Ingress <input name="host" placeholder="geko.example.org" autocomplete="off"><small>Empty: any host.</small></label>
      <label>Size of the volume <input name="storage" value="20Gi" autocomplete="off"></label>
      <label>Storage class <input name="storageClass" placeholder="the default of the cluster" autocomplete="off"></label>
      <label>Access mode
        <select name="accessMode">
          <option value="ReadWriteMany">ReadWriteMany: every module reads it from any node</option>
          <option value="ReadWriteOnce">ReadWriteOnce: a cluster of one node (minikube, kind)</option>
        </select>
      </label>
    </fieldset>

    <fieldset>
      <legend>Version</legend>
      <label>Tag of the images <input name="tag" value="latest" autocomplete="off"><small>Pin it, for example 0.2.0.</small></label>
      <details>
        <summary>Another registry</summary>
        <label>Prefix of the modules <input name="imagePrefix" value="ghcr.io/candymaps/geko-" autocomplete="off"><small>Followed by the name of the module.</small></label>
        <label>Image of the operator <input name="operatorImage" placeholder="ghcr.io/candymaps/geko:&lt;tag&gt;" autocomplete="off"></label>
      </details>
    </fieldset>

    <fieldset>
      <legend>Modules</legend>
      <div class="wizard-module" data-module="geocoder">
        <label class="wizard-check"><input type="checkbox" name="geocoder-enabled" checked> Geocoder</label>
        <div class="wizard-grid">
          <label>Replicas <input name="geocoder-replicas" value="2" inputmode="numeric"></label>
          <label>CPU <input name="geocoder-cpu" value="500m"></label>
          <label>Memory <input name="geocoder-memory" value="4Gi"></label>
          <label>Build memory <input name="geocoder-buildMemory" value="6Gi"></label>
          <label>Build CPU <input name="geocoder-buildCpu" placeholder="optional"></label>
        </div>
      </div>
      <div class="wizard-module" data-module="tiles">
        <label class="wizard-check"><input type="checkbox" name="tiles-enabled" checked> Tiles</label>
        <div class="wizard-grid">
          <label>Replicas <input name="tiles-replicas" value="2" inputmode="numeric"></label>
          <label>CPU <input name="tiles-cpu" placeholder="optional"></label>
          <label>Memory <input name="tiles-memory" value="1Gi"></label>
          <label>Build memory <input name="tiles-buildMemory" value="5Gi"></label>
          <label>Build CPU <input name="tiles-buildCpu" placeholder="optional"></label>
        </div>
      </div>
    </fieldset>

    <fieldset>
      <legend>Scope</legend>
      <div class="wizard-country">
        <label class="wizard-check"><input type="checkbox" name="scope-ES" checked> Spain (ES)</label>
        <label>Regions <input name="regions-ES" placeholder="ES-PM, ES-CT" autocomplete="off"><small>Empty: the whole country.</small></label>
      </div>
      <div class="wizard-country">
        <label class="wizard-check"><input type="checkbox" name="scope-LU"> Luxembourg (LU)</label>
        <label>Regions <input name="regions-LU" autocomplete="off"><small>Empty: the whole country.</small></label>
      </div>
    </fieldset>

    <fieldset>
      <legend>Data</legend>
      <label class="wizard-check"><input type="checkbox" name="acceptLicenses"> I accept the licenses of the sources that geko downloads</label>
      <label>Days between refreshes <input name="maxAgeDays" value="30" inputmode="numeric"></label>
    </fieldset>

    <ul class="wizard-errors" id="wizard-errors" role="alert" hidden></ul>

    <footer>
      <button type="button" class="btn" id="wizard-cancel">Cancel</button>
      <button type="submit" class="btn btn-primary">Download the zip</button>
    </footer>
  </form>
</dialog>

<style>
  .wizard { width: min(40rem, calc(100vw - 2rem)); max-height: calc(100vh - 2rem); padding: 0; border: 1px solid #e3e7ec; border-radius: 8px; box-shadow: 0 12px 32px rgb(0 0 0 / 0.2); }
  .wizard::backdrop { background: rgb(0 0 0 / 0.4); }
  .wizard form { display: grid; gap: 1rem; padding: 1.25rem; }
  .wizard header { display: flex; align-items: center; justify-content: space-between; }
  .wizard h2 { margin: 0; font-size: 1.25rem; }
  .wizard-close { border: 0; background: none; font-size: 1.5rem; line-height: 1; cursor: pointer; }
  .wizard fieldset { display: grid; gap: 0.75rem; margin: 0; padding: 0.75rem 1rem 1rem; border: 1px solid #e3e7ec; border-radius: 6px; }
  .wizard legend { padding: 0 0.4rem; font-weight: 600; }
  .wizard label { display: grid; gap: 0.25rem; font-size: 0.875rem; }
  .wizard input:not([type="checkbox"]), .wizard select { width: 100%; padding: 0.4rem 0.5rem; border: 1px solid #c8ccd2; border-radius: 4px; font: inherit; }
  .wizard small { color: #6b7280; }
  .wizard .wizard-check { display: flex; align-items: center; gap: 0.5rem; font-weight: 600; }
  .wizard-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(7.5rem, 1fr)); gap: 0.5rem; margin-top: 0.5rem; }
  .wizard-module, .wizard-country { display: grid; gap: 0.25rem; padding-bottom: 0.5rem; }
  .wizard details { display: grid; gap: 0.5rem; }
  .wizard details label { margin-top: 0.5rem; }
  .wizard-errors { margin: 0; padding-left: 1.25rem; color: #d93025; font-size: 0.875rem; }
  .wizard footer { display: flex; justify-content: flex-end; gap: 0.5rem; }
  /* The primary of the dashboard in place of the purple of the theme: its gradient and shadow, in green, also on hover and pressed. */
  .wizard-btn.btn-primary, .wizard .btn-primary { background-color: #a4cc47; background-image: linear-gradient(#b3d85a, #a4cc47 55%, #9ac03f); color: #1b2a05; font-weight: 600; }
  .wizard-btn.btn-primary:hover, .wizard-btn.btn-primary:focus, .wizard .btn-primary:hover, .wizard .btn-primary:focus { background-color: #94bb3a; background-image: linear-gradient(#a6cc4e, #94bb3a 55%, #89ae35); color: #1b2a05; }
  .wizard-btn.btn-primary:active, .wizard .btn-primary:active { background-color: #89ae35; background-image: none; color: #1b2a05; }
</style>
<script src="{{ '/assets/js/geko-wizard.js' | relative_url }}"></script>
<script src="{{ '/assets/js/geko-wizard-ui.js' | relative_url }}"></script>
