# geko in a container: the binary, with no configuration and no module: geko downloads the modules
# of its configuration and they download their data (PROJECT.md §7.1). Mount your own geko.toml at
# /etc/geko/geko.toml and a volume at /var/lib/geko, where the downloads, the work files and the
# published bundles live:
#
#   docker build -t geko .
#   docker run -p 8080:8080 -v geko-data:/var/lib/geko -v ./geko.toml:/etc/geko/geko.toml geko
#
# geko comes alone, without modules: it downloads those of [modules] into /var/lib/geko/modules
# (the newest of its contract, or the fixed version), signed (docs/MODULES_PLAN.md).
#
# The default command is `up`: serve at once (503 until the first bundle is published) while it
# downloads and builds what the configuration asks for, and build again every [data] max_age_days,
# without a restart. `serve` only serves what is already in /var/lib/geko or, with `[serve] bundles_url`, what another
# machine published.
#
# The target `local` (develop/compose.yaml, `make start-dev`) is for development instead: geko and the
# modules of MODULES, built from their repositories (the build contexts `geocoder` and `tiles`)
# against the SDK of this tree, in the image, so that it never downloads one.

FROM rust:1.99.0-bookworm AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p geko-app && mkdir /out && cp target/release/geko /out/geko

# The binary alone. `make image` replaces this stage with the last local build
# (`--build-context binary=…`), and the compile stage is then skipped.
FROM scratch AS binary
COPY --from=build /out/geko /geko

# The repositories of the modules, given as build contexts of the same name (develop/compose.yaml);
# empty without them.
FROM scratch AS geocoder
FROM scratch AS tiles

# geko and the modules of MODULES (comma-separated), each as a process of its own, as
# GEKO_MODULES reads them. GEKO_MODULE_VERSION is the version they say they are, which goes into
# the bundles they build: <version>-<commit> from `make start-dev`.
FROM rust:1.99.0-bookworm AS build-local
ARG MODULES=geocoder
ARG GEKO_MODULE_VERSION=
# Not release (fat LTO, one codegen unit): every change would link it all again, for minutes.
ARG PROFILE=ci
WORKDIR /src
COPY . .
# An empty GEKO_MODULE_VERSION is unset: empty, it would be the version of the modules. Each module
# takes the SDK, geko-fetch and geko-artifact from this tree instead of its git dependency on the
# repository of the host, which is private and may not have them yet.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,id=geko-target-local,target=/src/target \
    --mount=type=bind,from=geocoder,target=/modules/geocoder \
    --mount=type=bind,from=tiles,target=/modules/tiles \
    set -e; \
    [ -n "$GEKO_MODULE_VERSION" ] || unset GEKO_MODULE_VERSION; \
    cargo build --profile $PROFILE --locked -p geko-app --bin geko; \
    mkdir -p /out/modules /work; cp target/$PROFILE/geko /out/geko; \
    for m in $(echo "$MODULES" | tr ',' ' '); do \
      [ -f /modules/$m/Cargo.toml ] || { echo "module $m: its repository is not the build context $m (develop/compose.yaml)" >&2; exit 1; }; \
      rm -rf /work/$m && cp -r /modules/$m /work/$m && cd /work/$m; \
      sed -i 's|^\(geko-[a-z-]*\) = { git = "https://github.com/candymaps/geko"[^}]*}|\1 = { path = "/src/crates/\1" }|' Cargo.toml; \
      cargo build --profile $PROFILE --bin geko-$m --target-dir /src/target/modules; \
      mkdir /out/modules/$m; cp /src/target/modules/$PROFILE/geko-$m /out/modules/$m/; \
      cp "$(find . -name module.toml -not -path './target/*' | head -1)" /out/modules/$m/; \
      cd /src; \
    done

FROM debian:bookworm-slim AS base
# Links the image to its repository in GitHub's registry.
LABEL org.opencontainers.image.source=https://github.com/candymaps/geko
LABEL org.opencontainers.image.description="geko: geocoder with OSM and official national sources"
# ca-certificates for the mirrors and buckets that are not reached through the certificates
# compiled into the binary; curl for the health check.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --home /var/lib/geko geko && mkdir -p /var/lib/geko /etc/geko && chown geko /var/lib/geko
USER geko
VOLUME /var/lib/geko
EXPOSE 8080
# `up` answers /health from the start, also while it builds.
HEALTHCHECK --interval=30s --start-period=30s CMD curl -fs http://127.0.0.1:8080/health || exit 1
ENTRYPOINT ["geko"]
# On every interface whatever the listen of geko.toml says: inside a container, 127.0.0.1 is
# reached from nowhere but the container itself.
CMD ["up", "--listen", "0.0.0.0:8080"]

FROM base AS local
COPY --from=build-local /out/geko /usr/local/bin/geko
COPY --from=build-local /out/modules /usr/lib/geko/modules
ENV GEKO_MODULES=/usr/lib/geko/modules
ARG VERSION=unknown
ARG REVISION=unknown
LABEL org.opencontainers.image.version=$VERSION
LABEL org.opencontainers.image.revision=$REVISION

# The image published, and that of `docker build` without a target: the last stage. The version of
# geko (that of Cargo.toml) and its commit are given by `make image` and the image workflow, last,
# so that a new version does not rebuild the layers above.
FROM base
COPY --from=binary /geko /usr/local/bin/geko
ARG VERSION=unknown
ARG REVISION=unknown
LABEL org.opencontainers.image.version=$VERSION
LABEL org.opencontainers.image.revision=$REVISION
