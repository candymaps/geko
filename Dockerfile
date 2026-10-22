# geko in a container: the binary and a sample configuration, and no country pack: geko downloads
# those of its scope (PROJECT.md §2.4). Mount your own geko.toml at
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
# modules of MODULES, built from this same tree, in the image, so that it never downloads one.

FROM rust:1.99.0-bookworm AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p geko-app --no-default-features && mkdir /out && cp target/release/geko /out/geko

# The binary alone. `make image` replaces this stage with the last local build
# (`--build-context binary=…`), and the compile stage is then skipped.
FROM scratch AS binary
COPY --from=build /out/geko /geko

# geko and the modules of MODULES (comma-separated), each as a process of its own, as
# GEKO_MODULES reads them. GEKO_MODULE_VERSION is the version they say they are, which goes into
# the bundles they build: <version>-<commit> from `make start-dev`.
FROM rust:1.99.0-bookworm AS build-local
ARG MODULES=geocoder
ARG GEKO_MODULE_VERSION=
WORKDIR /src
COPY . .
# An empty GEKO_MODULE_VERSION is unset: empty, it would be the version of the modules.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,id=geko-target-local,target=/src/target \
    set -e; \
    [ -n "$GEKO_MODULE_VERSION" ] || unset GEKO_MODULE_VERSION; \
    modules=$(echo "$MODULES" | tr ',' ' '); \
    packages=; for m in $modules; do packages="$packages -p geko-module-$m --bin geko-$m"; done; \
    cargo build --release --locked -p geko-app --no-default-features --bin geko $packages; \
    mkdir -p /out/modules; cp target/release/geko /out/geko; \
    for m in $modules; do \
      mkdir /out/modules/$m; cp target/release/geko-$m modules/geko-$m/module.toml /out/modules/$m/; \
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
COPY develop/geko.toml /etc/geko/geko.toml
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
