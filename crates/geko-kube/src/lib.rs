//! The only place that talks to a Kubernetes cluster (kube-rs): the operator mode of geko
//! (docs/OPERATOR_MODE_PLAN.md, step 10) hands it manifests as JSON and gets back what the cluster
//! has. What to create is decided elsewhere (`geko k8s render`): this crate applies, lists, deletes
//! and watches, with no knowledge of geko.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use futures::StreamExt;
use kube::api::{Api, ApiResource, DeleteParams, DynamicObject, GroupVersionKind, ListParams, Patch, PatchParams};
use kube::runtime::{WatchStreamExt, watcher};
use serde_json::Value;

/// The field manager of what the operator applies: its fields are the ones it overwrites.
const MANAGER: &str = "geko-operator";

/// What the operator may delete when the configuration no longer asks for it. The claim of the
/// volume is not here: it holds the bundles and the downloads, and no configuration is worth them.
/// The jobs are not either: which of them stay is for the operator to say (`Cluster::jobs`).
const PRUNABLE: [Kind; 4] = [Kind::Deployment, Kind::Service, Kind::CronJob, Kind::Ingress];

/// What the operator looks at for changes: what it may delete, and the jobs, whose end lets the
/// next build start.
const WATCHED: [Kind; 5] = [Kind::Deployment, Kind::Service, Kind::CronJob, Kind::Ingress, Kind::Job];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    PersistentVolumeClaim,
    Deployment,
    Service,
    CronJob,
    Ingress,
    Job,
}

impl Kind {
    fn parse(kind: &str) -> Option<Kind> {
        Some(match kind {
            "PersistentVolumeClaim" => Kind::PersistentVolumeClaim,
            "Deployment" => Kind::Deployment,
            "Service" => Kind::Service,
            "CronJob" => Kind::CronJob,
            "Ingress" => Kind::Ingress,
            "Job" => Kind::Job,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Kind::PersistentVolumeClaim => "PersistentVolumeClaim",
            Kind::Deployment => "Deployment",
            Kind::Service => "Service",
            Kind::CronJob => "CronJob",
            Kind::Ingress => "Ingress",
            Kind::Job => "Job",
        }
    }

    fn resource(self) -> ApiResource {
        let (group, version, plural) = match self {
            Kind::PersistentVolumeClaim => ("", "v1", "persistentvolumeclaims"),
            Kind::Deployment => ("apps", "v1", "deployments"),
            Kind::Service => ("", "v1", "services"),
            Kind::CronJob => ("batch", "v1", "cronjobs"),
            Kind::Ingress => ("networking.k8s.io", "v1", "ingresses"),
            Kind::Job => ("batch", "v1", "jobs"),
        };
        ApiResource::from_gvk_with_plural(&GroupVersionKind::gvk(group, version, self.name()), plural)
    }
}

/// A resource of the cluster, by what names it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Object {
    kind: Kind,
    pub name: String,
}

impl Object {
    /// The object a manifest describes; `None` for a kind this crate does not handle.
    pub fn of(manifest: &Value) -> Option<Object> {
        Some(Object { kind: Kind::parse(manifest["kind"].as_str()?)?, name: manifest["metadata"]["name"].as_str()?.to_owned() })
    }

    /// A job, by its name.
    pub fn job(name: &str) -> Object {
        Object { kind: Kind::Job, name: name.to_owned() }
    }

    pub fn kind(&self) -> &'static str {
        self.kind.name()
    }
}

/// A deployment as the cluster runs it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Deployed {
    pub name: String,
    pub image: Option<String>,
    pub replicas: i64,
    pub ready: i64,
}

/// A job as the cluster has it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JobState {
    pub name: String,
    pub succeeded: bool,
    pub failed: bool,
}

impl JobState {
    /// It ran to its end, well or not: it will not run again.
    pub fn finished(&self) -> bool {
        self.succeeded || self.failed
    }
}

/// A pod that serves, as the cluster has it: where it answers and whether it is ready.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PodState {
    pub name: String,
    pub ip: Option<String>,
    pub ready: bool,
}

pub struct Cluster {
    client: kube::Client,
    namespace: String,
}

impl Cluster {
    /// The cluster this process runs in (or that its kubeconfig names), in `namespace` or in the
    /// one of its pod.
    pub async fn connect(namespace: Option<String>) -> Result<Cluster> {
        let client = kube::Client::try_default().await.context("cannot reach the cluster")?;
        let namespace = namespace.unwrap_or_else(|| client.default_namespace().to_owned());
        Ok(Cluster { client, namespace })
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    fn api(&self, kind: Kind) -> Api<DynamicObject> {
        Api::namespaced_with(self.client.clone(), &self.namespace, &kind.resource())
    }

    /// The data of a config map, or `None` while it does not exist.
    pub async fn config_map(&self, name: &str) -> Result<Option<BTreeMap<String, String>>> {
        let api: Api<k8s_openapi::api::core::v1::ConfigMap> = Api::namespaced(self.client.clone(), &self.namespace);
        Ok(api.get_opt(name).await.with_context(|| format!("cannot read the config map {name}"))?.and_then(|c| c.data))
    }

    /// Makes the cluster have this manifest, taking the fields it names from whoever else set them.
    pub async fn apply(&self, manifest: &Value) -> Result<()> {
        let object = Object::of(manifest).context("a manifest of a kind the operator does not handle")?;
        let params = PatchParams::apply(MANAGER).force();
        self.api(object.kind)
            .patch(&object.name, &params, &Patch::Apply(manifest))
            .await
            .with_context(|| format!("cannot apply {} {}", object.kind(), object.name))?;
        Ok(())
    }

    /// What carries the label `selector` (`key=value`) and the operator may delete.
    pub async fn prunable(&self, selector: &str) -> Result<Vec<Object>> {
        let mut found = Vec::new();
        for kind in PRUNABLE {
            let list = self.api(kind).list(&ListParams::default().labels(selector)).await.with_context(|| format!("cannot list the {}", kind.name()))?;
            found.extend(list.items.into_iter().filter_map(|o| o.metadata.name).map(|name| Object { kind, name }));
        }
        Ok(found)
    }

    pub async fn delete(&self, object: &Object) -> Result<()> {
        // In the background, so that the pods of a job go with it.
        match self.api(object.kind).delete(&object.name, &DeleteParams::background()).await {
            Ok(_) => Ok(()),
            Err(kube::Error::Api(status)) if status.code == 404 => Ok(()),
            Err(e) => Err(e).with_context(|| format!("cannot delete {} {}", object.kind(), object.name)),
        }
    }

    /// The deployments with the label `selector`, with the first image of each and how many of their
    /// replicas are ready.
    pub async fn deployments(&self, selector: &str) -> Result<Vec<Deployed>> {
        let list = self.api(Kind::Deployment).list(&ListParams::default().labels(selector)).await.context("cannot list the deployments")?;
        Ok(list
            .items
            .into_iter()
            .filter_map(|d| {
                let name = d.metadata.name?;
                let image = d.data["spec"]["template"]["spec"]["containers"][0]["image"].as_str().map(str::to_owned);
                let replicas = d.data["spec"]["replicas"].as_i64().unwrap_or(1);
                let ready = d.data["status"]["readyReplicas"].as_i64().unwrap_or(0);
                Some(Deployed { name, image, replicas, ready })
            })
            .collect())
    }

    /// The pods with the label `selector` that a deployment keeps (not those of a job), the oldest
    /// first (then by name) so that they come in the same order at every look.
    pub async fn pods(&self, selector: &str) -> Result<Vec<PodState>> {
        let api: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let mut items = api.list(&ListParams::default().labels(selector)).await.context("cannot list the pods")?.items;
        items.sort_by(|a, b| (&a.metadata.creation_timestamp, &a.metadata.name).cmp(&(&b.metadata.creation_timestamp, &b.metadata.name)));
        Ok(items
            .into_iter()
            .filter(|p| p.metadata.owner_references.as_ref().is_some_and(|o| o.iter().any(|o| o.kind == "ReplicaSet")))
            .filter(|p| p.metadata.deletion_timestamp.is_none())
            .filter_map(|p| {
                let name = p.metadata.name?;
                let status = p.status.unwrap_or_default();
                let ready = status.conditions.is_some_and(|c| c.iter().any(|c| c.type_ == "Ready" && c.status == "True"));
                Some(PodState { name, ip: status.pod_ip, ready })
            })
            .collect())
    }

    /// The jobs with the label `selector`.
    pub async fn jobs(&self, selector: &str) -> Result<Vec<JobState>> {
        let list = self.api(Kind::Job).list(&ListParams::default().labels(selector)).await.context("cannot list the jobs")?;
        Ok(list
            .items
            .into_iter()
            .filter_map(|j| {
                let name = j.metadata.name?;
                let succeeded = j.data["status"]["succeeded"].as_i64().unwrap_or(0) > 0;
                let failed = j.data["status"]["conditions"].as_array().is_some_and(|c| c.iter().any(|c| c["type"] == "Failed" && c["status"] == "True"));
                Some(JobState { name, succeeded, failed })
            })
            .collect())
    }

    /// What changes in the config map, or in anything with the label `selector` that the operator
    /// makes (so that what someone removes or edits by hand is made again); see `Changes`.
    pub fn changes(&self, config_map: &str, selector: &str) -> Changes {
        let maps: Api<k8s_openapi::api::core::v1::ConfigMap> = Api::namespaced(self.client.clone(), &self.namespace);
        let maps = watcher(maps, watcher::Config::default().fields(&format!("metadata.name={config_map}"))).default_backoff().touched_objects().map(|_| ()).boxed();
        let owned = WATCHED.map(|kind| watcher(self.api(kind), watcher::Config::default().labels(selector)).default_backoff().touched_objects().map(|_| ()).boxed());
        Changes { stream: futures::stream::select_all(std::iter::once(maps).chain(owned)).boxed() }
    }
}

/// Changes of the cluster that may call for a new look: the watches report each object once at the
/// start, and again whenever it changes, and reconnect on their own.
pub struct Changes {
    stream: futures::stream::BoxStream<'static, ()>,
}

impl Changes {
    /// Waits for a change, and takes with it those that came at the same time: one look follows them
    /// all. Never returns if the watches ended.
    pub async fn next(&mut self) {
        if self.stream.next().await.is_none() {
            std::future::pending::<()>().await;
        }
        while futures::FutureExt::now_or_never(self.stream.next()).flatten().is_some() {}
    }
}
