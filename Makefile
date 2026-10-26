# Shortcuts for working on geko: `make help` lists them. Linux and macOS run it as is; Windows
# through make.cmd, which runs it inside WSL, where geko is built and its data live.

# Run from Windows, make starts without the login profile that puts cargo in the PATH.
export PATH := $(HOME)/.cargo/bin:$(PATH)
export CARGO_TARGET_DIR ?= $(HOME)/.cache/geko-target
# Without a C compiler (WSL without build-essential), Zig compiles and links, if it is there (AGENTS.md).
ifeq ($(shell command -v cc 2>/dev/null),)
ifneq ($(wildcard $(HOME)/.local/bin/zigcc),)
export CC := $(HOME)/.local/bin/zigcc
export AR := $(HOME)/.local/bin/zigar
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER := $(HOME)/.local/bin/zigcc
endif
endif
BIN    := $(CARGO_TARGET_DIR)/release/geko
# geko lives in /usr/local/bin, reads /etc/geko/geko.toml and keeps its data in /var/lib/geko.
LISTEN ?= 0.0.0.0:8080
GEKO   := /usr/local/bin/geko

IMAGE    ?= geko
# Where `make release` publishes the image, and the tags it gives it: the version of geko and the
# commit, besides `latest`.
REGISTRY ?= ghcr.io/candymaps/geko
# Without the CR of a checkout made on Windows.
VERSION  := $(shell tr -d '\r' < Cargo.toml | sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p')
# The commit, read from .git where git is not installed (WSL compiles a checkout made on Windows).
COMMIT   := $(shell git rev-parse --short HEAD 2>/dev/null || { \
              r=$$(sed -n 's|^ref: ||p' .git/HEAD 2>/dev/null); \
              if [ -z "$$r" ]; then cat .git/HEAD; else cat ".git/$$r" 2>/dev/null || sed -n "s| $$r\$$||p" .git/packed-refs; fi; \
            } | cut -c1-7)
# An image built with uncommitted changes says so in its revision; without git, it cannot tell.
DIRTY    := $(shell command -v git > /dev/null && { git diff --quiet HEAD 2>/dev/null || echo -dirty; })
# The glibc of the base of the image (debian:bookworm-slim): a binary that needs a newer one does
# not start in it.
IMAGE_GLIBC := 2.36
# Fails if the binary $(1) needs a newer glibc than the image has.
define glibc
need=$$(grep -aoE 'GLIBC_[0-9]+\.[0-9]+' $(1) | sed 's/GLIBC_//' | sort -Vu | tail -1); \
[ "$$(printf '%s\n' $$need $(IMAGE_GLIBC) | sort -V | tail -1)" = "$(IMAGE_GLIBC)" ] || { \
  echo "$(notdir $(1)) needs glibc $$need, newer than the $(IMAGE_GLIBC) of the image: it would not start." >&2; \
  echo "Build it on an older system, with Zig (AGENTS.md), or let docker build compile it: docker build -t $(IMAGE) ." >&2; exit 1; };
endef

# `make start-dev` (develop/compose.yaml): its port, its configuration, the modules it builds (by
# default those enabled with `<module> = true` in the [modules] of that configuration) and where
# their repositories are, each as geko-<module>: beside this one.
DEV_PORT    ?= 8080
DEV_MODULES_DIR ?= $(abspath ..)
DEV_CONFIG  ?= develop/geko.toml
DEV_MODULES ?= $(shell awk '/^\[/ { on = ($$1 == "[modules]") } on && $$2 == "=" && $$3 == "true" { print $$1 }' $(DEV_CONFIG) | paste -sd, -)
# The Planetiler profile of the tiles module (its jar and styles), which its ci/profile.sh builds
# in Docker and the module is compiled with.
DEV_TILES_PROFILE ?= $(CARGO_TARGET_DIR)/geko-dev-tiles-profile
DEV_COMPOSE := GEKO_DEV_CONFIG="$(abspath $(DEV_CONFIG))" GEKO_DEV_PORT=$(DEV_PORT) GEKO_DEV_MODULES=$(DEV_MODULES) GEKO_DEV_MODULES_DIR="$(DEV_MODULES_DIR)" \
               GEKO_DEV_TILES_PROFILE="$(DEV_TILES_PROFILE)" \
               GEKO_DEV_VERSION=$(VERSION)-$(COMMIT)$(DIRTY) docker compose -f develop/compose.yaml

.PHONY: help bin install serve up data image start-dev stop-dev release test test-changed

help: ## This list
	@grep -E "^[a-z-]+:.*## " $(MAKEFILE_LIST) | awk -F ":.*## " '{ printf "  make %-10s %s\n", $$1, $$2 }'
	@echo "  Variables: LISTEN=$(LISTEN) IMAGE=$(IMAGE) REGISTRY=$(REGISTRY)"

bin: ## Compile geko in release
	cargo build --release -p geko-app

install: bin ## Install geko, /etc/geko/geko.toml (that of make start-dev, if there is none) and /var/lib/geko
	cmp -s $(BIN) /usr/local/bin/geko || sudo install -m 755 $(BIN) /usr/local/bin/geko
	[ -f /etc/geko/geko.toml ] || sudo install -D -m 644 develop/geko.toml /etc/geko/geko.toml
	[ -d /var/lib/geko ] || sudo install -d -o $$(id -un) -g $$(id -gn) /var/lib/geko

serve: install ## Serve the published bundle on LISTEN (the demo page is at /)
	$(GEKO) serve --listen $(LISTEN)

up: install ## Serve, and build in the background what the configuration asks for (now and every max_age_days)
	$(GEKO) up --listen $(LISTEN)

data: install ## Download and build what the configuration asks for
	$(GEKO) build

image: ## Docker image IMAGE with geko alone, from a local release build (docker build compiles it instead): it downloads its modules
	cargo build --release -p geko-app
	@$(call glibc,$(BIN))
	rm -rf $(CARGO_TARGET_DIR)/image && mkdir -p $(CARGO_TARGET_DIR)/image
	cp $(BIN) $(CARGO_TARGET_DIR)/image/geko
	BUILDX_GIT_INFO=false docker build --build-context binary=$(CARGO_TARGET_DIR)/image --build-arg VERSION=$(VERSION) --build-arg REVISION=$(COMMIT)$(DIRTY) -t $(IMAGE) .

start-dev: ## Build geko and DEV_MODULES (their repositories in DEV_MODULES_DIR, with the SDK of this tree) inside Docker and (re)create the container of develop/compose.yaml with DEV_CONFIG
	@[ -n "$(DEV_MODULES)" ] || { echo "no module enabled in $(DEV_CONFIG): DEV_MODULES=geocoder,…" >&2; exit 1; }
	@docker rm -f geko-dev > /dev/null 2>&1 || true
	@mkdir -p "$(DEV_TILES_PROFILE)"
	@case ",$(DEV_MODULES)," in *,tiles,*) "$(DEV_MODULES_DIR)/geko-tiles/ci/profile.sh" "$(DEV_TILES_PROFILE)" ;; esac
	$(DEV_COMPOSE) up -d --build
	@echo "geko-dev ($(DEV_MODULES)) builds, then serves http://localhost:$(DEV_PORT)/ (demo) and /docs (API). Its log follows; Ctrl-C leaves it running."
	$(DEV_COMPOSE) logs -f

stop-dev: ## Stop the container of make start-dev, keeping its volume (geko-dev_data)
	$(DEV_COMPOSE) down

release: ## Compile, make the image and push it to REGISTRY (needs a clean tree and docker login ghcr.io)
	@git diff --quiet HEAD || { echo "uncommitted changes: the image would not be that of $(COMMIT)" >&2; exit 1; }
	$(MAKE) image IMAGE=$(REGISTRY):$(VERSION)-$(COMMIT)
	docker tag $(REGISTRY):$(VERSION)-$(COMMIT) $(REGISTRY):$(VERSION)
	docker tag $(REGISTRY):$(VERSION)-$(COMMIT) $(REGISTRY):latest
	docker push $(REGISTRY):$(VERSION)-$(COMMIT)
	docker push $(REGISTRY):$(VERSION)
	docker push $(REGISTRY):latest

test: ## Every check of the host (ci/test.sh); each module checks itself in its repository
	ci/test.sh

test-changed: ## Only the checks that what changed since BASE (default: where it leaves dev) can fail
	@args=$$(ci/affected.sh $(or $(BASE),$$(git merge-base HEAD origin/dev))) && if [ -z "$$args" ]; then echo "only documentation changed"; else echo "ci/test.sh $$args"; ci/test.sh $$args; fi
