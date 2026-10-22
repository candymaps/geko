# geko

The host of geko, a modular, multi-country geospatial engine: the `geko` binary (configuration, scope, download and loading of the modules, publication and reload of bundles, common HTTP) and the SDK its modules are written against. Each module lives in its own repository: the geocoder in [candymaps/geko-geocoder](https://github.com/candymaps/geko-geocoder) (with the country packs, the golden and its optimizations), tiles in [candymaps/geko-tiles](https://github.com/candymaps/geko-tiles).

The specification of the host is in [PROJECT.md](docs/PROJECT.md), and the contract with the modules in [MODULES_PLAN.md](docs/MODULES_PLAN.md). Read them before designing or implementing anything.

The roadmap is in PROJECT.md §14, the state in §18 and what is pending in §16. When a step is finished, update them. Delete each pending item when it is closed (whatever it teaches goes to PROJECT.md or here).

## Environment

- Building and testing (cargo, `ci/test.sh`) happen in WSL Debian; `geko` runs in Docker (README, «Installation and usage»: the `geko-data` volume is its `/var/lib/geko`). The code lives in each machine's repository folder, which in WSL is `/mnt/<drive>/…`. The repositories of the modules sit beside this one (`../geko-geocoder`, `../geko-tiles`): `make start-dev` builds them from there.
- Use `CARGO_TARGET_DIR=$HOME/.cache/geko-target` (outside `/mnt/c`, to build fast).
- On the `D:` machine there is no C compiler in WSL (`build-essential` is missing): Zig is the temporary compiler and linker, with `CC=~/.local/bin/zigcc AR=~/.local/bin/zigar CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=~/.local/bin/zigcc`. From PowerShell, run each WSL command from a script (`wsl -d Debian -- bash <script>`): the quotes and `$` of an inline command get mangled on the way.
- Fixed paths: the binary in `/usr/local/bin/geko`, the configuration in `/etc/geko/geko.toml` (with `accept_licenses = true`; `make install` copies `develop/geko.toml` the first time; the published image carries none, and geko stops without one; every option is in the README, «Configuration reference») and, beside it, that of each module (`geko-<module>.toml`), and the data in `/var/lib/geko`. There is no `--config` and no `[data] dir`: only `GEKO_CONFIG` and `GEKO_DATA` change them, for the checks.
- Modules: `GEKO_MODULES` (one directory per module, with its `module.toml`) runs a module as a process of its own or loads it as a library (`kind = "process" | "dylib"`), and then a module not there is an error, never a download. Without it, a module is downloaded into `/var/lib/geko/modules/<module>/<version>/` from `ghcr.io/candymaps/geko-<module>`: the newest of the contract of `geko`, or the one of `[modules.<module>] version` (`registry` changes where from), signed; `geko plan` says which and from where, and `geko purge modules` removes those no active module or kept bundle uses. A module can also link itself into the host for its own checks, through `geko_app::main`.
- The private key that signs the modules and the data packages is in `~/.config/geko/pack-signing.key` (and in the `GEKO_PACK_SIGNING_KEY` GitHub secret of each module); the public one, in `TRUSTED_KEYS` of `geko-artifact`. Never print it or copy it into the repository.
- Nothing of geko loose in `~`: everything generated goes under `/var/lib/geko`, never in the repository.
- Publishing: `publish.yml`, on every push to `main` (the merge of a pull request that passed the checks), publishes the image to ghcr.io (no module in it) if the push touches something that goes into it, and moves the git tag `sdk-<contract>` if the SDK, `geko-fetch` or `geko-artifact` changed: the modules take them as git dependencies at that tag. All with the version in `Cargo.toml` (`<version>-<commit>`). Raising the version in `[workspace.package]` is making a release: in addition, `<version>` and `latest` on the image and the git tag `v<version>` on the pushed commit, with its GitHub release. Each module publishes itself from its own repository.
- GitHub CI (`ci.yml`) checks every pull request: the whole `ci/test.sh` if it changes what the checks build (`ci/affected.sh`), nothing if it only changes documentation. Its `test` job and `protected.yml` (which fails a pull request by anyone but the owner (Dependabot may only raise the actions of `.github/workflows/`) that changes `LICENSE`, `CLA.md`, `CONTRIBUTING.md`, `README.md`, `AGENTS.md`, `CLAUDE.md`, `.github/`, `ci/`, or what sets how strict the checks are: `deny.toml`, `clippy.toml`, `.cargo/`, `rust-toolchain.toml`; it runs on `pull_request_target`, as it is in the base branch, so the pull request cannot change it) are the statuses that the protection of `main` and `dev` requires, with the branch up to date, so a push to `main` only publishes (`publish.yml`); `pr-comment.yml` says on the pull request whether the checks passed, failed or were not needed. Locally, by hand: `make test` (everything) or `make test-changed` (what the change can break).
- Checks, the same locally and in CI (GitHub only calls the script): `ci/test.sh [--variant code,contracts]` (PROJECT.md §19).

## Branches and commits (Gitflow)

- **Nothing is ever pushed directly to `main`**: no commits, no pushes.
- All new development starts from `dev`, in its own branch with a Gitflow name: `feature/<name>`, `bugfix/<name>`… It ends with a PR from that branch to `dev`.
- A release (a push to `main`) is always a PR from `dev` to `main`.
- All commits follow [Conventional Commits](https://www.conventionalcommits.org/): `<type>(<optional scope>): <description>`, with type `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci` or `chore`. A breaking change takes `!` after the type (`feat!: …`) or a `BREAKING CHANGE:` footer. The title of each PR follows the same format.

## Key rules

- The host knows a module only through the contract of `geko-module-sdk` (`Module`): it never depends on one, and `ci/test.sh` (`architecture`) checks it. If a type, a piece of data or a port only makes sense for a module, it lives in the module.
- The contract is small and additive. A change that breaks it raises `CONTRACT` (`module.rs`), and every module must follow: publish the SDK (the tag), raise it in the module, publish the module, then the host. A new requirement of the contract goes into its conformance suite (`geko_module_sdk::conformance::module`).
- Selective abstraction: only the ports of PROJECT.md §4 are traits. Everything else is plain code; its trait is extracted when a second real implementation appears.
- No concrete technology (ureq, axum, libloading…) is used outside its crate.
- Each module serves its routes under `/<module>/`, reads its own `geko-<module>.toml` and builds its own part of the bundle. Every option of a module is documented in its repository, not in `geko.toml` or the README of the host. A disabled module does not download, does not instantiate, does not open files and does not register routes.
- Only `geko serve` uses tokio; `plan`, `fetch` and `build` are synchronous (the `Downloader` uses `ureq`). `geko serve` never downloads sources.
- Everything is automatic: only what the active modules ask for is downloaded and built. Downloading requires `accept_licenses = true`. No download protected by a CAPTCHA or other access control is automated.
- Build incremental by input hash (PROJECT.md §8): each stage leaves a `<dir>.stamp.json` stamp and is skipped if neither its code nor its inputs change; `geko build --force` redoes everything. The build must be deterministic.
- Build and serving on different machines. Bundles are immutable and versioned; a bundle in use is never modified. Production serving on Linux.
- Downloaded code runs only with a valid signature.
- Licenses (PROJECT.md §6): the code is AGPL-3.0-only (`LICENSE`) and every outside contribution requires the CLA (`CLA.md`). Every module declares the licenses of what it downloads, and they go into the bundle manifest.
- A change to the adapters (`process.rs`, `dylib.rs`, `remote.rs`) or to the HTTP path is measured with a module and real data: the geocoder's `ci/test.sh --scope spain` with `GEKO_SPAIN_ADAPTERS=1`, only when asked. Never compare timings with a measurement from another time.

## Optimizations: what worked and what did not

Before optimizing, check this list so as not to repeat what was already discarded. When you try something, add it here with its measurement, whether it works or not. Those of the geocoder are in its repository.

**Worked:**
- `DylibModule` (2026-10-22, all of Spain, 16 concurrent requests): no cost beyond noise against the module linked in, while `ProcessModule` lost 19 % of throughput in search, 20 % in autocomplete and 55 % in reverse. Making it the default is postponed.
