//! `geko operator`: geko as the control plane of a cluster (docs/OPERATOR_MODE_PLAN.md, step 10).
//! It reads the config map `geko`, makes what `k8s::manifests` says (a deployment and a service per
//! module, the jobs that build them, the volume, the Ingress), deletes what the configuration no
//! longer asks for, and serves the dashboard with what the cluster runs, because no process has
//! every module mounted. It also starts the build of each module, one after the other (they share
//! the downloads), whenever what the build reads from the config map changes. It looks again whenever the config map or a deployment changes, and every
//! few minutes, so that what someone changed by hand goes back to what the configuration says.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::Context;
use geko_kube::{Cluster, Deployed, JobState, Object};
use geko_module_sdk::module::ModuleName;
use geko_server::{ModuleStatus, PodAnswer};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::k8s::{self, CONFIG_MAP, OPERATOR, PORT};

/// What marks the resources of the operator: only these are looked at, and only these are deleted.
const MANAGED_LABEL: &str = "app.kubernetes.io/managed-by";
/// On the pods of a deployment: the digest of the config map they were started with, so that a
/// change to it rolls them. They read their configuration once, when they start.
const CONFIG_HASH: &str = "geko.candymaps/config-hash";
/// On the jobs that build, the digest of what the build of the module reads (`build_hash`): a job
/// is made once for each, so a build that ran is not run again until that changes.
const BUILD_HASH: &str = "geko.candymaps/build";
/// The look that does not wait for a change, so that what someone changed by hand goes back.
const RESYNC: Duration = Duration::from_secs(300);
/// The look after one that failed.
const RETRY: Duration = Duration::from_secs(30);
/// How long the dashboard waits for a pod to answer, and the most it takes from the answer.
const POD_TIMEOUT: Duration = Duration::from_secs(3);
const POD_BODY: u64 = 1 << 20;

fn selector() -> String {
    format!("{MANAGED_LABEL}={OPERATOR}")
}

/// Runs until the dashboard fails or the process is stopped. `options` are those of the manifests;
/// the operator adds that the dashboard answers `/`.
pub fn run(config: &Config, mut options: k8s::Options, listen: SocketAddr, namespace: Option<String>) -> anyhow::Result<()> {
    options.dashboard = Some(OPERATOR.to_owned());
    tokio::runtime::Runtime::new()?.block_on(async {
        let cluster = Arc::new(Cluster::connect(namespace).await?);
        tracing::info!(namespace = cluster.namespace(), "operating");
        let known: Arc<RwLock<Vec<ModuleStatus>>> = Arc::default();
        let shown = Arc::clone(&known);
        let statuses: geko_server::Statuses = Arc::new(move || shown.read().map(|s| s.clone()).unwrap_or_default());
        let asker = Arc::clone(&cluster);
        let pods: geko_server::Pods = Arc::new(move |module, path| {
            let cluster = Arc::clone(&asker);
            Box::pin(async move { answers(&cluster, &module, &path).await })
        });
        tokio::select! {
            served = geko_server::serve_dashboard(listen, crate::access(config), statuses, pods) => served.context("the dashboard failed"),
            kept = keep(&cluster, &options, &known) => kept,
        }
    })
}

/// What each pod that serves `module` answers to a GET of `path`, in the order of `Cluster::pods`.
/// They are asked at once, and one that does not answer in time says so: it does not hold back the
/// others. A pod that is not ready is not asked.
async fn answers(cluster: &Cluster, module: &str, path: &str) -> Vec<PodAnswer> {
    let pods = match cluster.pods(&format!("app.kubernetes.io/name=geko,app.kubernetes.io/component={module}")).await {
        Ok(pods) => pods,
        Err(e) => {
            tracing::warn!(error = format!("{e:#}"), module, "cannot list the pods");
            return Vec::new();
        }
    };
    let asked: Vec<_> = pods
        .into_iter()
        .map(|pod| {
            let url = pod.ip.as_ref().map(|ip| format!("http://{ip}:{PORT}/{path}"));
            tokio::spawn(async move {
                let body = match (url, pod.ready) {
                    (Some(url), true) => match tokio::task::spawn_blocking(move || geko_fetch::get_text(&url, POD_TIMEOUT, POD_BODY)).await {
                        Ok(Ok(text)) => serde_json::from_str(&text).map_err(|e| format!("the answer is not JSON: {e}")),
                        Ok(Err(e)) => Err(e.to_string()),
                        Err(e) => Err(e.to_string()),
                    },
                    (_, false) => Err("not ready".to_owned()),
                    (None, true) => Err("no address yet".to_owned()),
                };
                PodAnswer { pod: pod.name, ready: pod.ready, body }
            })
        })
        .collect();
    let mut answers = Vec::with_capacity(asked.len());
    for handle in asked {
        if let Ok(answer) = handle.await {
            answers.push(answer);
        }
    }
    answers
}

async fn keep(cluster: &Cluster, options: &k8s::Options, known: &RwLock<Vec<ModuleStatus>>) -> anyhow::Result<()> {
    let mut changes = cluster.changes(CONFIG_MAP, &selector());
    loop {
        let wait = match reconcile(cluster, options, known).await {
            Ok(()) => RESYNC,
            Err(e) => {
                tracing::error!(error = format!("{e:#}"), "cannot make the cluster what the configuration says; trying again");
                RETRY
            }
        };
        tokio::select! {
            () = changes.next() => {}
            () = tokio::time::sleep(wait) => {}
        }
    }
}

async fn reconcile(cluster: &Cluster, options: &k8s::Options, known: &RwLock<Vec<ModuleStatus>>) -> anyhow::Result<()> {
    let Some(data) = cluster.config_map(CONFIG_MAP).await? else {
        tracing::warn!("the config map {CONFIG_MAP} does not exist yet: nothing to make");
        return Ok(());
    };
    let text = data.get("geko.toml").with_context(|| format!("the config map {CONFIG_MAP} has no geko.toml"))?;
    let config = Config::parse(text).context("the geko.toml of the config map is not valid")?;
    let desired = managed(k8s::manifests(&config, options), &config_hash(&data));
    let builds = builds(&config.active_modules(), &data)?;
    let mut failed = 0;
    for resource in &desired {
        if let Err(e) = cluster.apply(resource).await {
            failed += 1;
            tracing::error!(error = format!("{e:#}"), "cannot apply");
        }
    }
    for object in stale(&cluster.prunable(&selector()).await?, &desired) {
        tracing::info!(kind = object.kind(), name = object.name, "deleted: the configuration no longer asks for it");
        cluster.delete(&object).await?;
    }
    let jobs = cluster.jobs(&selector()).await?;
    for job in obsolete(&jobs, &builds) {
        tracing::info!(name = job.name, "build job deleted: what it built from changed, or its module is off");
        cluster.delete(&Object::job(&job.name)).await?;
    }
    if let Some(build) = next_build(&builds, &jobs) {
        let cronjob = desired.iter().find(|r| r["kind"] == "CronJob" && r["metadata"]["name"] == format!("geko-build-{}", build.module));
        if let Some(cronjob) = cronjob {
            tracing::info!(module = %build.module, job = build.job, "building");
            cluster.apply(&build_job(cronjob, build)).await?;
        }
    }
    let deployed = cluster.deployments(&selector()).await?;
    if let Ok(mut shown) = known.write() {
        *shown = statuses(&config.active_modules(), &deployed, &builds, &jobs);
    }
    anyhow::ensure!(failed == 0, "{failed} resources could not be applied");
    Ok(())
}

/// The resources with the mark that says the operator made them, and the pods that serve with the
/// digest of the configuration they run with.
fn managed(mut resources: Vec<Value>, hash: &str) -> Vec<Value> {
    for resource in &mut resources {
        resource["metadata"]["labels"][MANAGED_LABEL] = json!(OPERATOR);
        if resource["kind"] == "Deployment" {
            resource["spec"]["template"]["metadata"]["annotations"] = json!({ CONFIG_HASH: hash });
        }
    }
    resources
}

/// A digest of every file of the config map: `geko.toml` and the `geko-<module>.toml` beside it.
fn config_hash(data: &BTreeMap<String, String>) -> String {
    let mut digest = Sha256::new();
    for (name, text) in data {
        digest.update(format!("{}:{name}{}:{text}", name.len(), text.len()));
    }
    hex::encode(&digest.finalize()[..8])
}

/// What the cluster has of the operator's that the configuration does not ask for.
fn stale(existing: &[Object], desired: &[Value]) -> Vec<Object> {
    let wanted: Vec<Object> = desired.iter().filter_map(Object::of).collect();
    existing.iter().filter(|o| !wanted.contains(o)).cloned().collect()
}

/// What the dashboard shows of each active module: the version its image says, and why it answers
/// nothing if no pod is ready.
fn statuses(modules: &[ModuleName], deployed: &[Deployed], builds: &[Build], jobs: &[JobState]) -> Vec<ModuleStatus> {
    modules
        .iter()
        .map(|module| {
            let name = module.to_string();
            let Some(d) = deployed.iter().find(|d| d.name == format!("geko-{name}")) else {
                return ModuleStatus { name, version: String::new(), unavailable: Some("not created yet".into()) };
            };
            let version = d.image.as_deref().and_then(tag).unwrap_or_default().to_owned();
            let job = builds.iter().find(|b| b.module == *module).and_then(|b| jobs.iter().find(|j| j.name == b.job));
            let unavailable = (d.ready == 0).then(|| match job {
                Some(j) if j.failed => format!("its build failed; no pod ready (0/{})", d.replicas),
                Some(j) if !j.succeeded => format!("building its bundle; no pod ready (0/{})", d.replicas),
                _ => format!("no pod ready (0/{})", d.replicas),
            });
            ModuleStatus { name, version, unavailable }
        })
        .collect()
}

/// The build that a module is owed: one job for each digest of what the build reads.
#[derive(Debug, PartialEq, Eq)]
struct Build {
    module: ModuleName,
    job: String,
    hash: String,
}

/// The builds that the configuration asks for, in the order of the modules.
fn builds(modules: &[ModuleName], data: &BTreeMap<String, String>) -> anyhow::Result<Vec<Build>> {
    modules
        .iter()
        .map(|module| {
            let hash = build_hash(module, data)?;
            Ok(Build { module: module.clone(), job: format!("geko-build-{module}-{}", &hash[..8]), hash })
        })
        .collect()
}

/// A digest of what the build of a module reads from the config map: everything but how it is served
/// (`[serve]`), run (`[deploy]`) or what the other modules are (`[modules]`), and its own
/// `geko-<module>.toml`. Another replica or another module's key does not rebuild it.
fn build_hash(module: &ModuleName, data: &BTreeMap<String, String>) -> anyhow::Result<String> {
    let text = data.get("geko.toml").context("the config map has no geko.toml")?;
    let mut table: toml::Table = toml::from_str(text).context("the geko.toml of the config map is not valid")?;
    table.remove("serve");
    table.remove("deploy");
    if let Some(toml::Value::Table(modules)) = table.get_mut("modules") {
        modules.retain(|name, _| name == module.as_str());
    }
    let own = data.get(&format!("geko-{module}.toml")).map_or("", String::as_str);
    let mut digest = Sha256::new();
    for part in [table.to_string().as_str(), own] {
        digest.update(format!("{}:{part}", part.len()));
    }
    Ok(hex::encode(&digest.finalize()[..8]))
}

/// The build to start now: the first that has no job yet, once every one before it has ended, as
/// they share the downloads. None while one runs, and when all have run.
fn next_build<'a>(builds: &'a [Build], jobs: &[JobState]) -> Option<&'a Build> {
    for build in builds {
        match jobs.iter().find(|j| j.name == build.job) {
            None => return Some(build),
            Some(job) if job.finished() => {}
            Some(_) => return None,
        }
    }
    None
}

/// The jobs of the operator that no build asks for: of what the build read before, or of a module
/// now off.
fn obsolete<'a>(jobs: &'a [JobState], builds: &[Build]) -> Vec<&'a JobState> {
    jobs.iter().filter(|j| !builds.iter().any(|b| b.job == j.name)).collect()
}

/// The job of a build: what the cron job of its module does, now, once.
fn build_job(cronjob: &Value, build: &Build) -> Value {
    let mut labels = cronjob["metadata"]["labels"].clone();
    labels[BUILD_HASH] = json!(build.hash);
    json!({
        "apiVersion": "batch/v1",
        "kind": "Job",
        "metadata": { "name": build.job, "labels": labels },
        "spec": cronjob["spec"]["jobTemplate"]["spec"].clone(),
    })
}

/// The tag of an image (`ghcr.io/candymaps/geko-tiles:1.2.0`), not the port of its registry.
fn tag(image: &str) -> Option<&str> {
    image.rsplit_once(':').map(|(_, tag)| tag).filter(|tag| !tag.contains('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(kind: &str, name: &str) -> Object {
        Object::of(&json!({ "kind": kind, "metadata": { "name": name } })).unwrap()
    }

    fn deployed(name: &str, image: &str, replicas: i64, ready: i64) -> Deployed {
        Deployed { name: name.into(), image: Some(image.into()), replicas, ready }
    }

    fn options() -> k8s::Options {
        k8s::Options {
            image_prefix: "ghcr.io/candymaps/geko-".into(),
            tag: "latest".into(),
            image: None,
            storage: "20Gi".into(),
            storage_class: None,
            access_mode: "ReadWriteMany".into(),
            dashboard: Some(OPERATOR.into()),
            host: None,
        }
    }

    #[test]
    fn everything_it_makes_carries_its_mark() {
        let config = Config::parse("[modules]\ngeocoder = true\ntiles = true\n").unwrap();
        let resources = managed(k8s::manifests(&config, &options()), "");
        assert!(!resources.is_empty());
        for resource in &resources {
            assert_eq!(resource["metadata"]["labels"][MANAGED_LABEL], json!(OPERATOR), "{}", resource["kind"]);
            assert_eq!(resource["metadata"]["labels"]["app.kubernetes.io/name"], json!("geko"), "its own labels stay");
        }
    }

    #[test]
    fn a_change_to_the_config_map_rolls_the_pods_that_serve() {
        let config = Config::parse("[modules]
geocoder = true
").unwrap();
        let template = |hash: &str| {
            let resources = managed(k8s::manifests(&config, &options()), hash);
            resources.iter().find(|r| r["kind"] == "Deployment").unwrap()["spec"]["template"]["metadata"]["annotations"].clone()
        };
        assert_eq!(template("abc"), json!({ CONFIG_HASH: "abc" }));
        let files = |text: &str| BTreeMap::from([("geko.toml".to_owned(), text.to_owned())]);
        assert_eq!(config_hash(&files("a")), config_hash(&files("a")));
        assert_ne!(config_hash(&files("a")), config_hash(&files("b")));
        let split = BTreeMap::from([("a".to_owned(), "bc".to_owned())]);
        let joined = BTreeMap::from([("ab".to_owned(), "c".to_owned())]);
        assert_ne!(config_hash(&split), config_hash(&joined), "where a name ends matters");
    }

    #[test]
    fn a_module_turned_off_leaves_what_it_made_to_delete_but_not_the_volume() {
        let both = Config::parse("[modules]\ngeocoder = true\ntiles = true\n").unwrap();
        let one = Config::parse("[modules]\ngeocoder = true\ntiles = false\n").unwrap();
        let existing: Vec<Object> = managed(k8s::manifests(&both, &options()), "").iter().filter_map(Object::of).collect();
        let gone = stale(&existing, &managed(k8s::manifests(&one, &options()), ""));
        let names: Vec<(&str, &str)> = gone.iter().map(|o| (o.kind(), o.name.as_str())).collect();
        assert_eq!(names, [("Deployment", "geko-tiles"), ("Service", "geko-tiles"), ("CronJob", "geko-build-tiles")]);
        assert!(stale(&existing, &managed(k8s::manifests(&both, &options()), "")).is_empty(), "what is asked for stays");
    }

    #[test]
    fn what_the_configuration_does_not_name_is_stale() {
        let desired = managed(k8s::manifests(&Config::parse("").unwrap(), &options()), "");
        assert_eq!(stale(&[object("Service", "geko-old")], &desired), [object("Service", "geko-old")]);
        assert!(Object::of(&json!({ "kind": "Secret", "metadata": { "name": "x" } })).is_none());
    }

    #[test]
    fn the_dashboard_shows_the_version_and_why_a_module_answers_nothing() {
        let modules: Vec<ModuleName> = ["geocoder", "tiles", "extra"].iter().map(|m| m.parse().unwrap()).collect();
        let cluster = [deployed("geko-geocoder", "ghcr.io/candymaps/geko-geocoder:1.2.0", 3, 2), deployed("geko-tiles", "localhost:5000/geko-tiles", 1, 0)];
        let shown = statuses(&modules, &cluster, &[], &[]);
        assert_eq!(shown[0], ModuleStatus { name: "geocoder".into(), version: "1.2.0".into(), unavailable: None });
        assert_eq!(shown[1], ModuleStatus { name: "tiles".into(), version: String::new(), unavailable: Some("no pod ready (0/1)".into()) });
        assert_eq!(shown[2].unavailable.as_deref(), Some("not created yet"));
    }

    fn data(toml: &str, files: &[(&str, &str)]) -> BTreeMap<String, String> {
        let mut data = BTreeMap::from([("geko.toml".to_owned(), toml.to_owned())]);
        data.extend(files.iter().map(|(name, text)| (name.to_string(), text.to_string())));
        data
    }

    fn module(name: &str) -> ModuleName {
        name.parse().unwrap()
    }

    #[test]
    fn a_build_runs_again_only_when_what_it_reads_changes() {
        let base = "[modules]\ngeocoder = true\ntiles = true\n[[scope]]\ncountry = \"LU\"\n[deploy.geocoder]\nreplicas = 1\n";
        let hash = |toml: &str, files: &[(&str, &str)]| build_hash(&module("geocoder"), &data(toml, files)).unwrap();
        let before = hash(base, &[]);
        assert_eq!(before.len(), 16);
        assert_eq!(hash(&base.replace("replicas = 1", "replicas = 5"), &[]), before, "how many pods serve is not what it builds");
        assert_eq!(hash(&format!("{base}[serve]\nlisten = \"0.0.0.0:80\"\n"), &[]), before, "nor how it serves");
        assert_eq!(hash(&base.replace("tiles = true", "tiles = false"), &[]), before, "nor another module being off");
        assert_eq!(hash(base, &[("geko-tiles.toml", "x = 1")]), before, "nor the files of another module");
        assert_ne!(hash(&base.replace("LU", "ES"), &[]), before, "the scope is");
        assert_ne!(hash(base, &[("geko-geocoder.toml", "x = 1")]), before, "and its own file");
        assert_ne!(hash(&base.replace("geocoder = true", "geocoder = false"), &[]), before, "and its being on");
    }

    #[test]
    fn builds_start_one_after_the_other_and_not_again() {
        let modules = [module("geocoder"), module("tiles")];
        let wanted = builds(&modules, &data("[modules]\ngeocoder = true\ntiles = true\n", &[])).unwrap();
        let job = |name: &str, succeeded: bool, failed: bool| JobState { name: name.into(), succeeded, failed };
        assert!(wanted[0].job.starts_with("geko-build-geocoder-") && wanted[1].job.starts_with("geko-build-tiles-"));
        assert_eq!(next_build(&wanted, &[]), Some(&wanted[0]), "the first one first");
        assert_eq!(next_build(&wanted, &[job(&wanted[0].job, false, false)]), None, "while it runs, none");
        assert_eq!(next_build(&wanted, &[job(&wanted[0].job, true, false)]), Some(&wanted[1]), "then the next");
        assert_eq!(next_build(&wanted, &[job(&wanted[0].job, false, true)]), Some(&wanted[1]), "a build that failed does not hold the rest");
        let all = [job(&wanted[0].job, true, false), job(&wanted[1].job, true, false)];
        assert_eq!(next_build(&wanted, &all), None, "what ran is not run again");
        assert_eq!(obsolete(&all, &wanted[..1]).iter().map(|j| j.name.as_str()).collect::<Vec<_>>(), [wanted[1].job.as_str()], "a module off leaves its job to delete");
        assert!(obsolete(&[job("geko-build-geocoder-old", true, false)], &wanted).len() == 1, "and so does a build of what it read before");
    }

    #[test]
    fn the_job_of_a_build_is_what_its_cron_job_does() {
        let config = Config::parse("[modules]\ngeocoder = true\ntiles = false\n").unwrap();
        let resources = managed(k8s::manifests(&config, &options()), "");
        let cronjob = resources.iter().find(|r| r["kind"] == "CronJob").unwrap();
        let build = Build { module: module("geocoder"), job: "geko-build-geocoder-1234abcd".into(), hash: "1234abcd".into() };
        let job = build_job(cronjob, &build);
        assert_eq!((job["kind"].as_str(), job["metadata"]["name"].as_str()), (Some("Job"), Some("geko-build-geocoder-1234abcd")));
        assert_eq!(job["metadata"]["labels"][MANAGED_LABEL], json!(OPERATOR), "it is the operator's, so that it can delete it");
        assert_eq!(job["metadata"]["labels"][BUILD_HASH], json!("1234abcd"));
        assert_eq!(job["spec"]["template"]["spec"]["containers"][0]["args"], json!(["build", "--module", "geocoder"]));
    }

    #[test]
    fn the_dashboard_says_when_a_module_is_still_building() {
        let modules = [module("geocoder")];
        let cluster = [deployed("geko-geocoder", "geko:dev", 1, 0)];
        let wanted = builds(&modules, &data("[modules]\ngeocoder = true\n", &[])).unwrap();
        let job = |succeeded, failed| [JobState { name: wanted[0].job.clone(), succeeded, failed }];
        let why = |jobs: &[JobState]| statuses(&modules, &cluster, &wanted, jobs)[0].unavailable.clone().unwrap();
        assert_eq!(why(&job(false, false)), "building its bundle; no pod ready (0/1)");
        assert_eq!(why(&job(false, true)), "its build failed; no pod ready (0/1)");
        assert_eq!(why(&job(true, false)), "no pod ready (0/1)");
        assert_eq!(why(&[]), "no pod ready (0/1)");
    }

    #[test]
    fn the_tag_is_not_the_port_of_the_registry() {
        assert_eq!(tag("geko:dev"), Some("dev"));
        assert_eq!(tag("registry:5000/geko"), None);
        assert_eq!(tag("registry:5000/geko:2"), Some("2"));
        assert_eq!(tag("geko"), None);
    }
}
