//! geko, the host (docs/MODULES_PLAN.md), as a library: its binary runs it with no module linked
//! in, and a module may run it with itself linked in, for its own checks (the golden, valgrind).

mod bundle;
mod bundle_store;
mod config;
mod dylib;
mod k8s;
mod modules;
mod operator;
mod progress;
mod remote;
mod run;
mod store;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use clap::{Parser, Subcommand};
use geko_module_sdk::module::{Module, ModuleName};

use crate::config::{Config, DeployMode};
pub use crate::modules::Linked;
use crate::modules::{Opened, Opener, Origin};

#[derive(Parser)]
#[command(name = "geko", version, about = "Geocoder and vector tile server")]
struct Cli {
    /// Comma-separated active modules; replaces the `[modules]` section. One module (`--module
    /// geocoder`) is how a pod of the operator mode runs: it builds and serves only its own.
    #[arg(long, global = true, value_delimiter = ',', alias = "module")]
    modules: Option<Vec<ModuleName>>,
    /// Log every step of a build, instead of its progress every 2 seconds.
    #[arg(long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show which sources and data a build would use. Only small catalog metadata is downloaded.
    Plan,
    /// Download the data needed by the active modules and enabled sources.
    Fetch {
        /// Check the sources for changes now, even if `[data] max_age_days` has not passed.
        #[arg(long)]
        refresh: bool,
    },
    /// Download what is missing or changed, then rebuild what changed and publish a bundle.
    Build {
        /// Rebuild every stage, even those whose code and inputs did not change.
        #[arg(long)]
        force: bool,
        /// First remove the downloads and work files of countries, regions and sources that the
        /// configuration no longer asks for.
        #[arg(long)]
        purge: bool,
        /// Check the sources for changes now, even if `[data] max_age_days` has not passed.
        #[arg(long)]
        refresh: bool,
    },
    /// Serve the active modules.
    Serve {
        #[arg(long)]
        listen: Option<SocketAddr>,
    },
    /// Serve, and build in the background what the configuration asks for: now, and again when
    /// a module asks for it (the geocoder: every `[data] max_age_days`). The bundle being served
    /// stays until the new one is published.
    Up {
        #[arg(long)]
        force: bool,
        /// As in `build`.
        #[arg(long)]
        purge: bool,
        /// As in `build`.
        #[arg(long)]
        refresh: bool,
        #[arg(long)]
        listen: Option<SocketAddr>,
    },
    /// Remove what the configuration no longer asks for; without a subcommand, all of it. Published
    /// bundles stay: `[build] keep` retires them.
    Purge {
        #[command(subcommand)]
        what: Option<Purge>,
    },
    /// Run as the operator of a Kubernetes cluster (`[deploy] mode = "operator"`): make the
    /// deployments, services, build jobs and Ingress of the active modules from the config map
    /// `geko`, keep them so, and serve the dashboard with what the cluster runs. Needs no module.
    Operator {
        #[command(flatten)]
        manifests: ManifestArgs,
        #[arg(long, default_value = "0.0.0.0:8080")]
        listen: SocketAddr,
        /// The namespace of the config map and of what it makes; by default that of the pod.
        #[arg(long)]
        namespace: Option<String>,
    },
    /// What the operator mode puts in a Kubernetes cluster.
    K8s {
        #[command(subcommand)]
        what: K8s,
    },
    /// A command of an active module: `geko geocoder query "Calle Mayor 5"`; `geko <module>` lists
    /// them.
    #[command(external_subcommand)]
    Module(Vec<String>),
}

/// What decides the manifests of the operator mode besides the configuration.
#[derive(clap::Args, Clone)]
struct ManifestArgs {
    /// The image of a module is this prefix followed by its name.
    #[arg(long, default_value = "ghcr.io/candymaps/geko-")]
    image_prefix: String,
    #[arg(long, default_value = "latest")]
    tag: String,
    /// One image for every module (the one of `make start-dev`), instead of the prefix and the tag.
    #[arg(long)]
    image: Option<String>,
    /// Size of the volume of bundles and downloads, which every module shares.
    #[arg(long, default_value = "20Gi")]
    storage: String,
    #[arg(long)]
    storage_class: Option<String>,
    /// How the volume is mounted: `ReadWriteMany` so that every module reaches it from any node;
    /// `ReadWriteOnce` for a cluster of one node (kind) whose storage offers no other.
    #[arg(long, default_value = "ReadWriteMany")]
    access_mode: String,
    /// The host name that the Ingress answers for; any when none.
    #[arg(long)]
    host: Option<String>,
}

impl ManifestArgs {
    fn options(&self) -> k8s::Options {
        k8s::Options {
            image_prefix: self.image_prefix.clone(),
            tag: self.tag.clone(),
            image: self.image.clone(),
            storage: self.storage.clone(),
            storage_class: self.storage_class.clone(),
            access_mode: self.access_mode.clone(),
            dashboard: None,
            host: self.host.clone(),
        }
    }
}

#[derive(Subcommand)]
enum K8s {
    /// Write the manifests for the active modules and `[deploy]` of the configuration to the
    /// standard output: a deployment and a service per module, a cron job that builds it, the
    /// volume they share and the Ingress. Needs no module downloaded, nor a cluster.
    Render {
        #[command(flatten)]
        manifests: ManifestArgs,
    },
    /// Write the manifests that install the operator (`geko operator`) in a cluster: its account
    /// and permissions, its deployment and the service of its dashboard. It makes the rest.
    Install {
        #[command(flatten)]
        manifests: ManifestArgs,
        /// The image that runs the operator; by default `ghcr.io/candymaps/geko:<tag>`, the one
        /// with no module in it.
        #[arg(long)]
        operator_image: Option<String>,
    },
}

#[derive(Subcommand, Clone, Copy)]
enum Purge {
    /// The downloads and work files of the countries, regions and sources that the configuration
    /// no longer asks for (what `build --purge` removes, without building).
    Data,
    /// The data packages the modules downloaded (the country packs of the geocoder) for the
    /// countries outside the scope.
    Packages {
        /// All of them: they are downloaded again when a build needs them.
        #[arg(long)]
        all: bool,
    },
    /// The downloaded versions of the modules that neither the configuration nor any bundle kept
    /// needs.
    Modules,
}

fn serve(config: &Config, opener: &Arc<Opener>, opened: &[Opened], listen: Option<SocketAddr>) -> anyhow::Result<()> {
    let mounted = opened.iter().map(|o| geko_server::Mounted::new(Arc::clone(&o.module))).collect();
    let origins: Vec<(ModuleName, Origin)> = opened.iter().map(|o| (o.module.describe().name, o.origin.clone())).collect();
    let reopener = Arc::clone(opener);
    let reopen: geko_server::Reopen = Arc::new(move |name, version| {
        let Some((_, origin)) = origins.iter().find(|(n, _)| n == name) else { return Ok(None) };
        reopener.reopen(name, version, origin)
    });
    // Before each look for a new version the server asks its store to bring it: nothing for a
    // directory (the build writes where the server reads), a pull for a mirror.
    let store = bundle_store::open(config);
    let pull: geko_server::Pull = {
        let store = Arc::clone(&store);
        Arc::new(move || {
            if let Some(version) = store.sync()? {
                tracing::info!(%version, from = %store.describe(), "bundle pulled");
            }
            Ok(())
        })
    };
    let bundles = geko_server::BundleSource {
        dir: store.dir().to_owned(),
        reload: std::time::Duration::from_secs(config.serve.reload_secs.max(1)),
        pull: Some(pull),
        reopen: Some(reopen),
    };
    let addr = listen.unwrap_or(config.serve.listen);
    let contact = Some(config.serve.attribution_contact.clone()).filter(|c| !c.is_empty());
    tokio::runtime::Runtime::new()?.block_on(geko_server::serve(addr, mounted, Some(bundles), access(config), contact)).context("server failed")
}

/// Who may call the API and how often, from `[serve]`.
fn access(config: &Config) -> geko_server::Access {
    let s = &config.serve;
    geko_server::Access {
        cors_origins: s.cors_origins.clone(),
        api_keys: s.api_keys.clone(),
        rate_per_s: s.rate_limit_per_s,
        burst: if s.rate_limit_burst > 0.0 { s.rate_limit_burst } else { s.rate_limit_per_s * 2.0 },
    }
}

/// Every active module prepares what the commands that build need (the geocoder, its country
/// packs and sources).
fn prepare(modules: &[Arc<dyn Module>]) -> anyhow::Result<()> {
    for m in modules {
        print!("{}", m.prepare()?);
    }
    Ok(())
}

fn purge(modules: &[Arc<dyn Module>], what: geko_module_sdk::module::Purge) -> anyhow::Result<()> {
    for m in modules {
        print!("{}", m.purge(what)?);
    }
    Ok(())
}

/// The (module, version) pairs still needed: those of the active modules and those that built a
/// part of a bundle kept.
fn modules_in_use(config: &Config, modules: &[Arc<dyn Module>]) -> Vec<(String, String)> {
    let mut used: Vec<(String, String)> = modules.iter().map(|m| m.describe()).map(|d| (d.name.to_string(), d.version)).collect();
    if let Ok(entries) = std::fs::read_dir(config.bundles_dir()) {
        for dir in entries.filter_map(|e| e.ok().map(|e| e.path())) {
            if let Ok(manifest) = geko_module_sdk::bundle::BundleManifest::read(&dir) {
                used.extend(manifest.modules.into_iter().map(|m| (m.name.to_string(), m.version)));
            }
        }
    }
    used
}

/// `geko <module> <command> …`: a command of an active module.
fn module_command(modules: &[Arc<dyn Module>], args: &[String]) -> anyhow::Result<()> {
    let [name, rest @ ..] = args else { unreachable!("clap gives the name of the subcommand") };
    let Some(module) = modules.iter().find(|m| m.describe().name.as_str() == name) else {
        let active: Vec<String> = modules.iter().map(|m| m.describe().name.to_string()).collect();
        anyhow::bail!("unknown command `{name}`: neither a command of geko (geko --help) nor an active module ({})", active.join(", "));
    };
    let Some((command, args)) = rest.split_first() else {
        let described = module.describe();
        println!("Commands of the {name} module (geko {name} <command> --help):");
        for c in &described.commands {
            println!("  {:<12} {}", c.name, c.about);
        }
        return Ok(());
    };
    print!("{}", module.command(command, args)?);
    Ok(())
}

/// In `[deploy] mode = "operator"` no process runs every module (docs/OPERATOR_MODE_PLAN.md): the
/// commands that would fail with what to do instead, rather than run in the other mode without a
/// word. One module per process (`--module`) is how the pods that the operator makes run. `geko
/// operator` itself only makes sense in that mode.
fn check_deploy(config: &Config, cli: &Cli) -> anyhow::Result<()> {
    let runs_every_module = matches!(cli.command, Command::Build { .. } | Command::Serve { .. } | Command::Up { .. });
    if config.deploy.mode == DeployMode::Operator && runs_every_module && cli.modules.is_none() {
        anyhow::bail!(
            "[deploy] mode = \"operator\": a process runs one module (`geko serve --module geocoder`, \
             `geko build --module geocoder`), and `geko operator` makes those in a cluster. \
             Set mode = \"single\" (the default) to run every module in this one"
        );
    }
    if config.deploy.mode != DeployMode::Operator && matches!(cli.command, Command::Operator { .. }) {
        anyhow::bail!("`geko operator` needs [deploy] mode = \"operator\" in the configuration");
    }
    Ok(())
}

/// Runs geko with the command line of this process and `linked`, the modules linked into it, which
/// `GEKO_MODULES` comes before and the downloads after (`modules`).
pub fn main(linked: Vec<Linked>) -> anyhow::Result<()> {
    let cli = Cli::parse();
    if cli.verbose {
        // SAFETY: nothing else runs yet. The modules, loaded into this process, read it.
        unsafe { std::env::set_var(geko_module_sdk::progress::VERBOSE, "1") };
    }
    use tracing_subscriber::prelude::*;
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .finish()
        .with(tracing_subscriber::filter::filter_fn(geko_module_sdk::progress::shown))
        .init();

    let path = config::config_path();
    anyhow::ensure!(
        path.exists(),
        "no configuration at {}: write one (README, «Configuration reference»), or name another with GEKO_CONFIG; in Docker, mount it at {}",
        path.display(),
        config::CONFIG_PATH
    );
    let config = Config::load(&path)?;
    check_deploy(&config, &cli)?;
    match &cli.command {
        Command::K8s { what: K8s::Render { manifests } } => {
            print!("{}", k8s::render(&config, &manifests.options()));
            return Ok(());
        }
        Command::K8s { what: K8s::Install { manifests, operator_image } } => {
            let image = operator_image.clone().unwrap_or_else(|| format!("ghcr.io/candymaps/geko:{}", manifests.tag));
            print!("{}", k8s::install(&manifests.options(), &image));
            return Ok(());
        }
        Command::Operator { manifests, listen, namespace } => {
            return operator::run(&config, manifests.options(), *listen, namespace.clone());
        }
        _ => {}
    }
    let active = modules::resolve(&config, cli.modules.as_deref())?;
    let mut opener = Opener::new(&config, &path, linked, modules::local()?);
    if std::env::var_os("GEKO_MODULES").is_some() {
        opener = opener.without_downloads();
    }
    let opener = Arc::new(opener);
    let opened = active.iter().map(|name| opener.open(name)).collect::<geko_module_sdk::Result<Vec<_>>>()?;
    let modules: Vec<Arc<dyn Module>> = opened.iter().map(|o| Arc::clone(&o.module)).collect();
    // Naming the modules (`--module geocoder`) is building some of them: the bundle keeps the part
    // of the others that the current one has.
    let keep_others = cli.modules.is_some();

    match cli.command {
        Command::Plan => {
            println!("Modules (contract {}):", geko_module_sdk::module::CONTRACT);
            for o in &opened {
                let d = o.module.describe();
                println!("  {} {}: {}", d.name, d.version, o.origin);
            }
            prepare(&modules)?;
            for m in &modules {
                print!("{}", m.plan()?);
            }
        }
        Command::Fetch { refresh } => {
            prepare(&modules)?;
            for m in &modules {
                print!("{}", m.fetch(refresh)?);
            }
        }
        Command::Build { force, purge: first_purge, refresh } => {
            prepare(&modules)?;
            if first_purge {
                purge(&modules, geko_module_sdk::module::Purge::Data)?;
            }
            print!("{}", run::build(&config, &modules, force, refresh, keep_others)?);
        }
        Command::Serve { listen } => serve(&config, &opener, &opened, listen)?,
        Command::Up { force, purge: first_purge, refresh, listen } => {
            prepare(&modules)?;
            if first_purge {
                purge(&modules, geko_module_sdk::module::Purge::Data)?;
            }
            // Fails now rather than serve nothing while every build in the background fails.
            for m in &modules {
                m.ready()?;
            }
            let (builder, background) = (config.clone(), modules.clone());
            std::thread::Builder::new().name("geko-build".into()).spawn(move || run::regenerate(&builder, &background, force, refresh, keep_others))?;
            serve(&config, &opener, &opened, listen)?;
        }
        Command::Purge { what } => {
            if matches!(what, None | Some(Purge::Data)) {
                prepare(&modules)?;
                purge(&modules, geko_module_sdk::module::Purge::Data)?;
            }
            if let None | Some(Purge::Packages { .. }) = what {
                let all = matches!(what, Some(Purge::Packages { all: true }));
                purge(&modules, geko_module_sdk::module::Purge::Packages { all })?;
            }
            if let None | Some(Purge::Modules) = what {
                for dir in opener.store().purge(&modules_in_use(&config, &modules))? {
                    println!("purged {}", dir.display());
                }
            }
        }
        Command::K8s { .. } | Command::Operator { .. } => unreachable!("handled above, before any module is opened"),
        Command::Module(args) => module_command(&modules, &args)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked(toml: &str, args: &[&str]) -> anyhow::Result<()> {
        let cli = Cli::try_parse_from(std::iter::once("geko").chain(args.iter().copied())).unwrap();
        check_deploy(&Config::parse(toml).unwrap(), &cli)
    }

    #[test]
    fn operator_mode_asks_for_one_module_per_process() {
        let operator = "[deploy]\nmode = \"operator\"\n";
        for command in ["serve", "build", "up"] {
            let err = checked(operator, &[command]).unwrap_err().to_string();
            assert!(err.contains("--module"), "{command}: {err}");
            assert!(checked(operator, &[command, "--module", "geocoder"]).is_ok(), "{command}");
        }
        assert!(checked(operator, &["plan"]).is_ok());
        assert!(checked(operator, &["--modules", "geocoder,tiles", "serve"]).is_ok());
    }

    #[test]
    fn the_operator_runs_only_in_the_operator_mode() {
        let err = checked("", &["operator"]).unwrap_err().to_string();
        assert!(err.contains("mode = \"operator\""), "{err}");
        assert!(checked("[deploy]
mode = \"operator\"
", &["operator"]).is_ok());
        assert!(checked("[deploy]
mode = \"operator\"
", &["k8s", "install"]).is_ok());
    }

    #[test]
    fn single_mode_runs_every_module() {
        assert!(checked("", &["serve"]).is_ok());
        assert!(checked("[deploy]\nmode = \"single\"\n", &["up"]).is_ok());
    }
}
