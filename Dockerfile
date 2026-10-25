# syntax=docker/dockerfile:1.27@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e
# Stages: dev is the development environment, check is the single gate, cli-build compiles
# the Hub client CLI and cli is its image. The SDK itself is a library and ships no image.

FROM rust:1.99.0-trixie@sha256:6ff07edce8775d0f64be7aba9197229407301bddf2054d62c27b541a6238a181 AS dev
# clang and cmake build aws-lc-sys, which jsonwebtoken's AWS-LC backend links. PostgreSQL
# 18.6 from the PGDG repository, whose signing key Debian's postgresql-common ships, runs
# the private clusters of the operations and usage tests; the packages create none.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates clang cmake git pkg-config procps postgresql-common \
    && /usr/share/postgresql-common/pgdg/apt.postgresql.org.sh -y \
    && mkdir -p /etc/postgresql-common/createcluster.d \
    && printf 'create_main_cluster = false\n' > /etc/postgresql-common/createcluster.d/veoveo.conf \
    && apt-get install -y --no-install-recommends \
        postgresql-18=18.6-1.pgdg13+2 postgresql-client-18=18.6-1.pgdg13+2 libpq5=18.6-1.pgdg13+2 \
    && rm -rf /var/lib/apt/lists/* /var/log/apt /var/log/dpkg.log \
    && rustup component add clippy rustfmt
ENV PATH=/usr/lib/postgresql/18/bin:${PATH}
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    cargo install --locked \
        cargo-nextest@0.9.148 \
        cargo-deny@0.20.2 \
        cargo-hack@0.6.45 \
        cargo-semver-checks@0.51.0
COPY env/profile.d/rust.sh /etc/profile.d/rust.sh
# A development environment installs toolchains, components and crates, so the sandbox user owns them.
RUN useradd --create-home --uid 1000 --shell /bin/bash sandbox \
    && chown -R sandbox:sandbox /usr/local/rustup /usr/local/cargo
USER sandbox

FROM dev AS source
WORKDIR /home/sandbox/src
COPY --chown=sandbox:sandbox . .

FROM source AS check
# The check and cli-build steps keep Cargo's registry, its Git checkouts and the workspace
# target in BuildKit cache mounts of the build host, so consecutive checks reuse compiled
# crates. They run as the sandbox user, so these caches are its own, apart from the
# root-owned registry cache of the dev stage's cargo install. Factory builds never use them.
# cargo-hack builds every feature of every crate alone, which proves that the contract
# feature compiles without the server runtime; a test checks its dependency graph.
RUN --mount=type=cache,id=veoveo-hub-sdk-cargo-registry,target=/usr/local/cargo/registry,uid=1000,sharing=locked \
    --mount=type=cache,id=veoveo-hub-sdk-cargo-git,target=/usr/local/cargo/git,uid=1000,sharing=locked \
    --mount=type=cache,id=veoveo-hub-sdk-target,target=/home/sandbox/src/target,uid=1000,sharing=locked \
    cargo fetch --locked \
    && cargo fmt --check \
    && cargo clippy --locked --all-targets --all-features -- -D warnings \
    && cargo deny --locked check \
    && cargo hack check --each-feature --no-dev-deps \
    && cargo nextest run --locked --profile ci

FROM source AS cli-build
# target/ is a cache mount, so the binary leaves it in the same step.
RUN --mount=type=cache,id=veoveo-hub-sdk-cargo-registry,target=/usr/local/cargo/registry,uid=1000,sharing=locked \
    --mount=type=cache,id=veoveo-hub-sdk-cargo-git,target=/usr/local/cargo/git,uid=1000,sharing=locked \
    --mount=type=cache,id=veoveo-hub-sdk-target,target=/home/sandbox/src/target,uid=1000,sharing=locked \
    cargo build --locked --release --bin veoveo-hub-sdk-cli \
    && cp target/release/veoveo-hub-sdk-cli /home/sandbox/veoveo-hub-sdk-cli

# The CLI alone. An installation mounts this image read-only into the workloads that use
# it, where the binary runs on the C library of the image it is mounted into.
FROM scratch AS cli
COPY --from=cli-build --chmod=0755 /home/sandbox/veoveo-hub-sdk-cli /usr/local/bin/veoveo-hub-sdk-cli
