# Why does this exist?
# Debian doesn't provide prebuilt musl packages
# rocksdb requires a prebuilt liburing, and linking fails if a gnu one is provided

ARG RUST_VERSION=1
ARG ALPINE_VERSION=3.22

FROM --platform=$BUILDPLATFORM docker.io/tonistiigi/xx AS xx
FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-alpine${ALPINE_VERSION} AS base
FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-alpine${ALPINE_VERSION} AS toolchain

# Install repo tools and dependencies
RUN --mount=type=cache,target=/etc/apk/cache apk add \
    build-base pkgconfig make jq bash \
    curl git file \
    llvm-dev clang clang-static lld


# Developer tool versions
# renovate: datasource=github-releases depName=cargo-bins/cargo-binstall
ENV BINSTALL_VERSION=1.17.5
# renovate: datasource=github-releases depName=psastras/sbom-rs
ENV CARGO_SBOM_VERSION=0.9.1
# renovate: datasource=crate depName=lddtree
ENV LDDTREE_VERSION=0.5.0

# Install unpackaged tools
RUN <<EOF
    set -o xtrace
    curl --retry 5 -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
    cargo binstall --no-confirm cargo-sbom --version $CARGO_SBOM_VERSION
    cargo binstall --no-confirm lddtree --version $LDDTREE_VERSION
EOF

# Set up xx (cross-compilation scripts)
COPY --from=xx / /
ARG TARGETPLATFORM

# Install libraries linked by the binary
RUN --mount=type=cache,target=/etc/apk/cache xx-apk add musl-dev gcc g++ liburing-dev

# Set up Rust toolchain
WORKDIR /app
COPY ./rust-toolchain.toml .
RUN rustc --version \
    && xx-cargo --setup-target-triple

# Build binary
# We disable incremental compilation to save disk space, as it only produces a minimal speedup for this case.
RUN echo "CARGO_INCREMENTAL=0" >> /etc/environment

# Configure pkg-config
RUN <<EOF
    set -o xtrace
    if command -v "$(xx-info)-pkg-config" >/dev/null 2>/dev/null; then
        echo "PKG_CONFIG_LIBDIR=/usr/lib/$(xx-info)/pkgconfig" >> /etc/environment
        echo "PKG_CONFIG=/usr/bin/$(xx-info)-pkg-config" >> /etc/environment
    fi
    echo "PKG_CONFIG_ALLOW_CROSS=true" >> /etc/environment
EOF

# Configure cc to use clang version
RUN <<EOF
    set -o xtrace
    echo "CC=clang" >> /etc/environment
    echo "CXX=clang++" >> /etc/environment
EOF

# Cross-language LTO
RUN <<EOF
    set -o xtrace
    echo "CFLAGS=-flto" >> /etc/environment
    echo "CXXFLAGS=-flto" >> /etc/environment
    # Linker is set to target-compatible clang by xx
    echo "RUSTFLAGS='-Clinker-plugin-lto -Clink-arg=-fuse-ld=lld'" >> /etc/environment
EOF

# Apply CPU-specific optimizations if TARGET_CPU is provided
ARG TARGET_CPU

RUN <<EOF
    set -o allexport
    set -o xtrace
    . /etc/environment
    if [ -n "${TARGET_CPU}" ]; then
        echo "CFLAGS='${CFLAGS} -march=${TARGET_CPU}'" >> /etc/environment
        echo "CXXFLAGS='${CXXFLAGS} -march=${TARGET_CPU}'" >> /etc/environment
        echo "RUSTFLAGS='${RUSTFLAGS} -C target-cpu=${TARGET_CPU}'" >> /etc/environment
    fi
EOF

# Prepare output directories
RUN mkdir /out

FROM toolchain AS builder


# Get source
COPY . .

ARG TARGETPLATFORM

# Verify environment configuration
RUN xx-cargo --print-target-triple

# Conduwuit version info
ARG GIT_COMMIT_HASH
ARG GIT_COMMIT_HASH_SHORT
ARG GIT_REMOTE_URL
ARG GIT_REMOTE_COMMIT_URL
ARG CONDUWUIT_VERSION_EXTRA
ARG CONTINUWUITY_VERSION_EXTRA
ENV GIT_COMMIT_HASH=$GIT_COMMIT_HASH
ENV GIT_COMMIT_HASH_SHORT=$GIT_COMMIT_HASH_SHORT
ENV GIT_REMOTE_URL=$GIT_REMOTE_URL
ENV GIT_REMOTE_COMMIT_URL=$GIT_REMOTE_COMMIT_URL
ENV CONDUWUIT_VERSION_EXTRA=$CONDUWUIT_VERSION_EXTRA
ENV CONTINUWUITY_VERSION_EXTRA=$CONTINUWUITY_VERSION_EXTRA

ARG RUST_PROFILE=release

# Build the binary
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    --mount=type=cache,target=/app/target,id=continuwuity-cargo-target-${TARGET_CPU}-${TARGETPLATFORM}-musl-${RUST_PROFILE} \
    bash <<'EOF'
    set -o allexport
    set -o xtrace
    . /etc/environment
    TARGET_DIR=($(cargo metadata --no-deps --format-version 1 | \
            jq -r ".target_directory"))
    mkdir /out/sbin
    PACKAGE=conduwuit
    xx-cargo build --locked --profile ${RUST_PROFILE} \
        -p $PACKAGE --no-default-features --features bindgen-static,release_max_log_level,standard;
    BINARIES=($(cargo metadata --no-deps --format-version 1 | \
        jq -r ".packages[] | select(.name == \"$PACKAGE\") | .targets[] | select( .kind | map(. == \"bin\") | any ) | .name"))
    for BINARY in "${BINARIES[@]}"; do
        echo $BINARY
        xx-verify $TARGET_DIR/$(xx-cargo --print-target-triple)/release/$BINARY
        cp $TARGET_DIR/$(xx-cargo --print-target-triple)/release/$BINARY /out/sbin/$BINARY
    done
EOF

# Generate Software Bill of Materials (SBOM)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    bash <<'EOF'
    set -o xtrace
    mkdir /out/sbom
    typeset -A PACKAGES
    for BINARY in /out/sbin/*; do
        BINARY_BASE=$(basename ${BINARY})
        package=$(cargo metadata --no-deps --format-version 1 | jq -r ".packages[] | select(.targets[] | select( .kind | map(. == \"bin\") | any ) | .name == \"$BINARY_BASE\") | .name")
        if [ -z "$package" ]; then
            continue
        fi
        PACKAGES[$package]=1
    done
    for PACKAGE in $(echo ${!PACKAGES[@]}); do
        echo $PACKAGE
        cargo sbom --cargo-package $PACKAGE > /out/sbom/$PACKAGE.spdx.json
    done
EOF

# Extract dynamically linked dependencies
RUN <<EOF
    set -o xtrace
    mkdir /out/libs
    mkdir /out/libs-root
    for BINARY in /out/sbin/*; do
        lddtree "$BINARY" | awk '{print $(NF-0) " " $1}' | sort -u -k 1,1 | awk '{print "install", "-D", $1, (($2 ~ /^\//) ? "/out/libs-root" $2 : "/out/libs/" $2)}' | xargs -I {} sh -c {}
    done
EOF

FROM scratch

WORKDIR /

# Copy root certs for tls into image
# You can also mount the certs from the host
# --volume /etc/ssl/certs:/etc/ssl/certs:ro
COPY --from=base /etc/ssl/certs /etc/ssl/certs

# Copy our build
COPY --from=builder /out/sbin/ /sbin/
# Copy SBOM
COPY --from=builder /out/sbom/ /sbom/

# Copy dynamic libraries to root
COPY --from=builder /out/libs-root/ /
COPY --from=builder /out/libs/ /usr/lib/

# Inform linker where to find libraries
ENV LD_LIBRARY_PATH=/usr/lib

# Continuwuity default port
EXPOSE 8008

CMD ["/sbin/conduwuit"]
