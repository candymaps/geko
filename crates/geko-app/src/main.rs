mod bundle;
mod config;
mod dylib;
mod modules;
#[cfg(unix)]
mod process;
mod remote;
mod run;
mod store;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use clap::{Parser, Subcommand};
use geko_module_sdk::module::{Module, ModuleName};

use crate::config::Config;
use crate::modules::{Opened, Opener, Origin};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Parser)]
#[command(name = "geko", version, about = "Geocoder and vector tile server")]
struct Cli {
    /// Comma-separated active modules; replaces the `[modules]` section.
    #[arg(long, global = true, value_delimiter = ',')]
    modules: Option<Vec<ModuleName>>,
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
    /// A command of an active module: `geko geocoder query "Calle Mayor 5"`; `geko <module>` lists
    /// them.
    #[command(external_subcommand)]
    Module(Vec<String>),
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
    let pull = (!config.serve.bundles_url.is_empty()).then(|| {
        let (url, dir, keep) = (config.serve.bundles_url.clone(), config.bundles_dir(), config.build.keep);
        // Bundles are not sources: a serving machine pulls them even when it never downloads data.
        let downloader = geko_fetch::HttpDownloader::new(geko_fetch::HttpDownloaderConfig::default());
        let pull: geko_server::Pull = Arc::new(move || {
            if let Some(version) = bundle::pull(&url, &dir, &downloader, keep)? {
                tracing::info!(%version, %url, "bundle pulled");
            }
            Ok(())
        });
        pull
    });
    let bundles = geko_server::BundleSource {
        dir: config.bundles_dir(),
        reload: std::time::Duration::from_secs(config.serve.reload_secs.max(1)),
        pull,
        reopen: Some(reopen),
    };
    let addr = listen.unwrap_or(config.serve.listen);
    let s = &config.serve;
    let access = geko_server::Access {
        cors_origins: s.cors_origins.clone(),
        api_keys: s.api_keys.clone(),
        rate_per_s: s.rate_limit_per_s,
        burst: if s.rate_limit_burst > 0.0 { s.rate_limit_burst } else { s.rate_limit_per_s * 2.0 },
    };
    let contact = Some(s.attribution_contact.clone()).filter(|c| !c.is_empty());
    tokio::runtime::Runtime::new()?.block_on(geko_server::serve(addr, mounted, Some(bundles), access, contact)).context("server failed")
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

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();
    let path = config::config_path();
    let config = if path.exists() { Config::load(&path)? } else { Config::default() };
    let active = modules::resolve(&config, cli.modules.as_deref())?;
    let mut opener = Opener::new(&config, &path, modules::linked(), modules::local()?);
    if std::env::var_os("GEKO_MODULES").is_some() {
        opener = opener.without_downloads();
    }
    let opener = Arc::new(opener);
    let opened = active.iter().map(|name| opener.open(name)).collect::<geko_module_sdk::Result<Vec<_>>>()?;
    let modules: Vec<Arc<dyn Module>> = opened.iter().map(|o| Arc::clone(&o.module)).collect();

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
            print!("{}", run::build(&config, &modules, force, refresh)?);
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
            std::thread::Builder::new().name("geko-build".into()).spawn(move || run::regenerate(&builder, &background, force, refresh))?;
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
        Command::Module(args) => module_command(&modules, &args)?,
    }
    Ok(())
}
