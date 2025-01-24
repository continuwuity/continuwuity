ARG RUST_VERSION=1.84

FROM --platform=$BUILDPLATFORM docker.io/tonistiigi/xx AS xx
FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-slim-bookworm AS base
FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-slim-bookworm AS builder

# Prevent deletion of apt cache
RUN rm -f /etc/apt/apt.conf.d/docker-clean

# Match Rustc version as close as possible
# rustc -vV
ARG LLVM_VERSION=19
ENV RUSTUP_TOOLCHAIN=${RUST_VERSION}

# Install repo tools
# Line one: compiler tools
# Line two: curl, for downloading binaries
# Line three: for xx-verify
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
apt-get update && apt-get install -y \
    clang-${LLVM_VERSION} lld-${LLVM_VERSION} pkg-config make \
    curl git \
    file

# Create symlinks for LLVM tools
RUN <<EOF
    ln -s /usr/bin/clang-${LLVM_VERSION} /usr/bin/clang
    ln -s "/usr/bin/clang-${LLVM_VERSION}++" "/usr/bin/clang++"
    ln -s /usr/bin/lld-${LLVM_VERSION} /usr/bin/lld
EOF

# Developer tool versions
# renovate: datasource=github-releases depName=cargo-bins/cargo-binstall
ENV BINSTALL_VERSION=1.10.21
# renovate: datasource=github-releases depName=psastras/sbom-rs
ENV CARGO_SBOM_VERSION=0.9.1
# renovate: datasource=crate depName=lddtree
ENV LDDTREE_VERSION=0.3.7

# Install unpackaged tools
RUN <<EOF
    curl --retry 5 -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
    cargo binstall --no-confirm cargo-sbom --version $CARGO_SBOM_VERSION
    cargo binstall --no-confirm lddtree --version $LDDTREE_VERSION
EOF

# Set up xx (cross-compilation scripts)
COPY --from=xx / /
ARG TARGETPLATFORM

# Install libraries linked by the binary
# xx-* are xx-specific meta-packages
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
xx-apt-get install -y \
    xx-c-essentials xx-cxx-essentials \
    liburing-dev

# Set up Rust toolchain
WORKDIR /app
COPY ./rust-toolchain.toml .
RUN rustc --version \
    && rustup target add $(xx-cargo --print-target-triple)

# Get source
COPY . .

# Build binary
# We disable incremental compilation to save disk space, as it only produces a minimal speedup for this case.
ENV CARGO_INCREMENTAL=0

# Configure pkg-config
RUN <<EOF
    echo "PKG_CONFIG_LIBDIR=/usr/lib/$(xx-info)/pkgconfig" >> /etc/environment
    echo "PKG_CONFIG=/usr/bin/$(xx-info)-pkg-config"
    echo "PKG_CONFIG_ALLOW_CROSS=true" >> /etc/environment
EOF

# Configure cc to use clang version
RUN <<EOF
    echo "CC=clang" >> /etc/environment
    echo "CXX=clang++" >> /etc/environment    
EOF

# Cross-language LTO
RUN <<EOF
    echo "CFLAGS=-flto" >> /etc/environment
    echo "CXXFLAGS=-flto" >> /etc/environment
    echo "RUSTFLAGS='-Clinker-plugin-lto -Clinker=clang -Clink-arg=-fuse-ld=lld'" >> /etc/environment
EOF

# Apply CPU-specific optimizations if TARGET_CPU is provided
ARG TARGET_CPU=
RUN <<EOF
  set -o allexport
  . /etc/environment
  if [ -n "${TARGET_CPU}" ]; then
    echo "CFLAGS='${CFLAGS} -march=${TARGET_CPU}'" >> /etc/environment
    echo "CXXFLAGS='${CXXFLAGS} -march=${TARGET_CPU}'" >> /etc/environment
    echo "RUSTFLAGS='${RUSTFLAGS} -C target-cpu=${TARGET_CPU}'" >> /etc/environment
  fi
EOF

# Conduwuit version info
ARG COMMIT_SHA=
ARG CONDUWUIT_VERSION_EXTRA=
ENV CONDUWUIT_VERSION_EXTRA=$CONDUWUIT_VERSION_EXTRA
RUN <<EOF
if [ -z "${CONDUWUIT_VERSION_EXTRA}" ]; then
    echo "CONDUWUIT_VERSION_EXTRA='$(set -e; git rev-parse --short ${COMMIT_SHA:-HEAD} || echo unknown) Jade Build'" >> /etc/environment
fi
EOF

# Verify environment configuration
RUN cat /etc/environment

# Prepare output directories
RUN mkdir /out

# Build the binary
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    --mount=type=cache,target=/app/target \
    bash <<EOF
  set -o allexport
  . /etc/environment
  xx-cargo build --locked --release
  xx-verify ./target/$(xx-cargo --print-target-triple)/release/conduwuit
  cp ./target/$(xx-cargo --print-target-triple)/release/conduwuit /out/app
EOF

# Generate Software Bill of Materials (SBOM)
RUN cargo sbom > /out/sbom.spdx.json

# Extract dynamically linked dependencies
# RUN lddtree /out/app
RUN lddtree /out/app | awk '{print $(NF-0) " " $1}' | sort -u -k 1,1 | awk '{print "install", "-D", $1, (($2 ~ /^\//) ? "/out/libs-root" $2 : "/out/libs/" $2)}'
RUN <<EOF
    mkdir /out/libs
    mkdir /out/libs-root
    lddtree /out/app | awk '{print $(NF-0) " " $1}' | sort -u -k 1,1 | awk '{print "install", "-D", $1, (($2 ~ /^\//) ? "/out/libs-root" $2 : "/out/libs/" $2)}' | xargs -I {} sh -c {}
EOF

FROM scratch

WORKDIR /

# Copy root certs for tls into image
# You can also mount the certs from the host
# --volume /etc/ssl/certs:/etc/ssl/certs:ro
COPY --from=base /etc/ssl/certs /etc/ssl/certs

# Copy our build
COPY --from=builder /out/app ./app
# Copy SBOM
COPY --from=builder /out/sbom.spdx.json ./sbom.spdx.json

# Copy dynamic libraries to root
COPY --from=builder /out/libs-root/ /
COPY --from=builder /out/libs/ /usr/lib/

# Inform linker where to find libraries
ENV LD_LIBRARY_PATH=/usr/lib

CMD ["/app"]
