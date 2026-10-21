mod config;
mod modules;
mod plan;
mod sources;
mod stages;

use std::net::SocketAddr;

use anyhow::Context;
use clap::{Parser, Subcommand};
use geko_server::ModuleName;

use crate::config::Config;

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
    /// Show how a query is interpreted (debugging aid).
    Parse {
        text: String,
        #[arg(long, default_value = "ES")]
        country: geko_core::country::CountryCode,
    },
    /// Search the built segment from the command line (debugging aid).
    Query {
        text: String,
        #[arg(long, default_value = "ES")]
        country: geko_core::country::CountryCode,
        /// Treat the last word as a prefix being typed.
        #[arg(long)]
        autocomplete: bool,
        /// Bias results towards "lat,lon".
        #[arg(long)]
        focus: Option<String>,
        #[arg(long, default_value_t = 10)]
        size: usize,
        /// Print the time of each interpretation of the query (stderr).
        #[arg(long)]
        debug: bool,
    },
    /// Reverse geocode a point from the command line (debugging aid).
    Reverse {
        lat: f64,
        lon: f64,
        #[arg(long, default_value = "ES")]
        country: geko_core::country::CountryCode,
        #[arg(long, default_value_t = 5)]
        size: usize,
    },
    /// Serve the active modules.
    Serve {
        #[arg(long)]
        listen: Option<SocketAddr>,
    },
    /// Serve, and build in the background what the configuration asks for: now, and again every
    /// `[data] max_age_days`. The bundle being served stays until the new one is published.
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
}

#[derive(Subcommand, Clone, Copy)]
enum Purge {
    /// The downloads and work files of the countries, regions and sources that the configuration
    /// no longer asks for (what `build --purge` removes, without building).
    Data,
    /// The downloaded country packs of the countries outside the scope.
    CountryPacks {
        /// All of them: they are downloaded again when a build needs them.
        #[arg(long)]
        all: bool,
    },
}

fn serve(config: &Config, active: &[geko_core::module::ModuleName], listen: Option<SocketAddr>) -> anyhow::Result<()> {
    let instances = active.iter().map(|m| modules::instantiate(*m, config)).collect::<Result<Vec<_>, _>>()?;
    let pull = (!config.serve.bundles_url.is_empty()).then(|| {
        let (url, dir, keep) = (config.serve.bundles_url.clone(), config.bundles_dir(), config.build.keep);
        // Bundles are not sources: a serving machine pulls them even when it never downloads data.
        let downloader = geko_fetch::HttpDownloader::new(geko_fetch::HttpDownloaderConfig::default());
        let pull: geko_server::Pull = std::sync::Arc::new(move || {
            if let Some(version) = geko_build::bundle::pull(&url, &dir, &downloader, keep)? {
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
    };
    let addr = listen.unwrap_or(config.serve.listen);
    let s = &config.serve;
    let access = geko_server::Access {
        cors_origins: s.cors_origins.clone(),
        api_keys: s.api_keys.clone(),
        rate_per_s: s.rate_limit_per_s,
        burst: if s.rate_limit_burst > 0.0 { s.rate_limit_burst } else { s.rate_limit_per_s * 2.0 },
    };
    tokio::runtime::Runtime::new()?.block_on(geko_server::serve(addr, instances, Some(bundles), access, Some(s.attribution_contact.clone()).filter(|c| !c.is_empty()))).context("server failed")
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();
    let path = config::config_path();
    let config = if path.exists() { Config::load(&path)? } else { Config::default() };
    // GEKO_PACKS names packs that are not downloaded: those of the repository, for the checks.
    let given_packs = std::env::var_os("GEKO_PACKS");
    let packs_dir = given_packs.clone().map_or_else(|| config.data.dir.join("packs"), std::path::PathBuf::from);
    geko_country_pack::init(packs_dir.clone());
    let active = modules::resolve(&config.modules, cli.modules.as_deref(), modules::compiled)?;
    // The commands that build download the packs of the scope first: what they prepare needs them.
    let ensure_packs = || -> anyhow::Result<()> {
        if given_packs.is_none() {
            print!("{}", plan::ensure_packs(&config, &packs_dir)?);
        }
        Ok(())
    };

    match cli.command {
        Command::Plan => {
            ensure_packs()?;
            let prepared = plan::prepare(&config, &active, &sources::registry())?;
            print!("{}", plan::plan(&config, &active, &prepared, &plan::downloader(&config, false)));
        }
        Command::Fetch { refresh } => {
            ensure_packs()?;
            let prepared = plan::prepare(&config, &active, &sources::registry())?;
            print!("{}", plan::fetch(&config, &prepared, &plan::downloader(&config, refresh))?);
        }
        Command::Build { force, purge, refresh } => {
            ensure_packs()?;
            let prepared = plan::prepare(&config, &active, &sources::registry())?;
            if purge {
                print!("{}", plan::purge(&config, &prepared)?);
            }
            print!("{}", plan::build(&config, &active, &prepared, &plan::downloader(&config, refresh), force)?);
        }
        Command::Parse { text, country } => {
            let parsed = sources::parser(country)?.parse(&text)?;
            for i in parsed.interpretations {
                println!("{:>5.1}%  {:?}", i.probability * 100.0, i.kind);
            }
        }
        Command::Query { text, country, autocomplete, focus, size, debug } => {
            let dir = plan::segment_dir(&config, country).join("geocode");
            let opened = std::time::Instant::now();
            let engine = sources::geocode_engine(&dir, sources::normalizer(country)?, false)?;
            let open_ms = opened.elapsed().as_secs_f64() * 1000.0;
            let parser = sources::parser(country)?;
            let mut ctx = geko_core::query::QueryContext { size, ..Default::default() };
            if let Some(f) = focus {
                let (lat, lon) = f.split_once(',').context("--focus must be lat,lon")?;
                ctx.focus = Some(geko_core::geo::Coord::from_degrees(lat.trim().parse()?, lon.trim().parse()?)?);
            }
            let mut hits = Vec::new();
            let started = std::time::Instant::now();
            let parsed = parser.parse(&text)?;
            if autocomplete {
                engine.autocomplete(&parsed, &ctx, &mut hits)?;
            } else {
                engine.search(&parsed, &ctx, &mut hits)?;
            }
            let cold_us = started.elapsed().as_micros();
            let runs = 200;
            let warm = std::time::Instant::now();
            for _ in 0..runs {
                let parsed = parser.parse(&text)?;
                if autocomplete { engine.autocomplete(&parsed, &ctx, &mut hits)? } else { engine.search(&parsed, &ctx, &mut hits)? }
            }
            let warm_us = warm.elapsed().as_micros() / runs;
            if debug {
                // Once more, warm, timing each interpretation.
                let debug_ctx = geko_core::query::QueryContext { debug: true, ..ctx.clone() };
                let parsed = parser.parse(&text)?;
                if autocomplete { engine.autocomplete(&parsed, &debug_ctx, &mut hits)? } else { engine.search(&parsed, &debug_ctx, &mut hits)? }
            }
            for h in &hits {
                let Some(place) = engine.place(h.id) else { continue };
                let props: serde_json::Value =
                    serde_json::from_slice(&[b"{".as_slice(), &place.json, b"}"].concat()).unwrap_or_default();
                println!(
                    "{:>7.3}  {:<13} {:<12} {}  [{}]",
                    h.score,
                    place.kind.as_str(),
                    h.match_type.as_str(),
                    props["label"].as_str().unwrap_or(&place.name),
                    place.public_id
                );
            }
            println!("open {open_ms:.1} ms · first query {cold_us} µs · warm {warm_us} µs/query");
        }
        Command::Reverse { lat, lon, country, size } => {
            let segment = plan::segment_dir(&config, country);
            let opened = std::time::Instant::now();
            let reverse = sources::reverse_index(&segment.join("reverse"))?;
            let engine = sources::geocode_engine(&segment.join("geocode"), sources::normalizer(country)?, false)?;
            let open_ms = opened.elapsed().as_millis();
            let point = geko_core::geo::Coord::from_degrees(lat, lon)?;
            let mut hits = Vec::new();
            let started = std::time::Instant::now();
            reverse.nearest(point, geko_core::query::KindFilter::ALL, size, &mut hits)?;
            let chain = reverse.admin_at(point);
            let query_us = started.elapsed().as_micros();
            for h in &hits {
                let Some(place) = engine.place(h.id) else { continue };
                let props: serde_json::Value = serde_json::from_slice(&[b"{".as_slice(), &place.json, b"}"].concat()).unwrap_or_default();
                let metres = geko_geom::distance_m((point.lon_e7(), point.lat_e7()), (place.point.lon_e7(), place.point.lat_e7()));
                println!("{metres:>7.1} m  {:<13} {}  [{}]", place.kind.as_str(), props["label"].as_str().unwrap_or(&place.name), place.public_id);
            }
            if let Some(chain) = chain {
                let levels: Vec<String> = chain.levels.iter().map(|l| format!("{} {} ({})", l.kind, l.name, l.code)).collect();
                println!("admin: {}", levels.join(" > "));
            }
            println!("open {open_ms} ms · query {query_us} µs");
        }
        Command::Serve { listen } => serve(&config, &active, listen)?,
        Command::Up { force, purge, refresh, listen } => {
            ensure_packs()?;
            let prepared = plan::prepare(&config, &active, &sources::registry())?;
            if purge {
                print!("{}", plan::purge(&config, &prepared)?);
            }
            // Fails now rather than serve nothing while every build in the background fails.
            plan::licenses_accepted(&config, &prepared)?;
            let (builder, background) = (config.clone(), active.clone());
            std::thread::Builder::new()
                .name("geko-build".into())
                .spawn(move || plan::regenerate(&builder, &background, &prepared, force, refresh))?;
            serve(&config, &active, listen)?;
        }
        Command::Purge { what } => {
            if matches!(what, None | Some(Purge::Data)) {
                ensure_packs()?;
                let prepared = plan::prepare(&config, &active, &sources::registry())?;
                print!("{}", plan::purge(&config, &prepared)?);
            }
            if let None | Some(Purge::CountryPacks { .. }) = what {
                let all = matches!(what, Some(Purge::CountryPacks { all: true }));
                let scope: Vec<_> = config.scope()?.countries().collect();
                for dir in geko_country_pack::registry::purge(&packs_dir, (!all).then_some(scope.as_slice()))? {
                    println!("purged {}", dir.display());
                }
            }
        }
    }
    Ok(())
}
