ARG RUST_VERSION=1

FROM --platform=$BUILDPLATFORM docker.io/tonistiigi/xx AS xx
FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-slim-bookworm AS base
FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-slim-bookworm AS toolchain

# Prevent deletion of apt cache
RUN rm -f /etc/apt/apt.conf.d/docker-clean

# Match Rustc version as close as possible
# rustc -vV
ARG LLVM_VERSION=19
# ENV RUSTUP_TOOLCHAIN=${RUST_VERSION}

# Install repo tools
# Line one: compiler tools
# Line two: curl, for downloading binaries
# Line three: for xx-verify
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
apt-get update && apt-get install -y \
    clang-${LLVM_VERSION} lld-${LLVM_VERSION} pkg-config make jq \
    curl git \
    file

# Create symlinks for LLVM tools
RUN <<EOF
    # clang
    ln -s /usr/bin/clang-${LLVM_VERSION} /usr/bin/clang
    ln -s "/usr/bin/clang++-${LLVM_VERSION}" "/usr/bin/clang++"
    # lld
    ln -s /usr/bin/ld64.lld-${LLVM_VERSION} /usr/bin/ld64.lld
    ln -s /usr/bin/ld.lld-${LLVM_VERSION} /usr/bin/ld.lld
    ln -s /usr/bin/lld-${LLVM_VERSION} /usr/bin/lld
    ln -s /usr/bin/lld-link-${LLVM_VERSION} /usr/bin/lld-link
    ln -s /usr/bin/wasm-ld-${LLVM_VERSION} /usr/bin/wasm-ld
EOF

# Developer tool versions
# renovate: datasource=github-releases depName=cargo-bins/cargo-binstall
ENV BINSTALL_VERSION=1.12.3
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
    xx-c-essentials xx-cxx-essentials pkg-config \
    liburing-dev

# Set up Rust toolchain
WORKDIR /app
COPY ./rust-toolchain.toml .
RUN rustc --version \
    && rustup target add $(xx-cargo --print-target-triple)

# Build binary
# We disable incremental compilation to save disk space, as it only produces a minimal speedup for this case.
RUN echo "CARGO_INCREMENTAL=0" >> /etc/environment

# Configure pkg-config
RUN <<EOF
    echo "PKG_CONFIG_LIBDIR=/usr/lib/$(xx-info)/pkgconfig" >> /etc/environment
    echo "PKG_CONFIG=/usr/bin/$(xx-info)-pkg-config" >> /etc/environment
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
    # Linker is set to target-compatible clang by xx
    echo "RUSTFLAGS='-Clinker-plugin-lto -Clink-arg=-fuse-ld=lld'" >> /etc/environment
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

# Prepare output directories
RUN mkdir /out

FROM toolchain AS builder


# Get source
COPY . .

# Conduwuit version info
ARG COMMIT_SHA=
ARG SHORT_COMMIT_SHA=
ARG REMOTE_URL=
ARG CONDUWUIT_VERSION_EXTRA=
ENV COMMIT_SHA=$COMMIT_SHA
ENV SHORT_COMMIT_SHA=$SHORT_COMMIT_SHA
ENV REMOTE_URL=$REMOTE_URL
ENV CONDUWUIT_VERSION_EXTRA=$CONDUWUIT_VERSION_EXTRA

# Calculate version info from git if not provided via ARGs
# and write all relevant vars to /etc/environment
RUN <<'EOF'
set -e # Exit on error

# Use temp variables to store calculated values
calculated_commit_sha=""
calculated_remote_url=""
calculated_version_extra=""

# --- COMMIT_SHA ---
# Calculate COMMIT_SHA if ENV var (from ARG) is empty
if [ -z "${COMMIT_SHA}" ]; then
    # Try to get short commit hash from git
    calculated_commit_sha=$(git rev-parse HEAD 2>/dev/null || echo "")
    if [ -n "${calculated_commit_sha}" ]; then
        echo "COMMIT_SHA='${calculated_commit_sha}'" >> /etc/environment
    fi
else
    # Ensure ARG-provided value is in /etc/environment
    echo "COMMIT_SHA='${COMMIT_SHA}'" >> /etc/environment
fi


if [ -z "${SHORT_COMMIT_SHA}" ]; then
    # Try to get short commit hash from git
    calculated_short_commit_sha=$(git rev-parse --short HEAD 2>/dev/null || echo "")
    if [ -n "${calculated_short_commit_sha}" ]; then
        echo "SHORT_COMMIT_SHA='${calculated_short_commit_sha}'" >> /etc/environment
    fi
else
    # Ensure ARG-provided value is in /etc/environment
    echo "SHORT_COMMIT_SHA='${SHORT_COMMIT_SHA}'" >> /etc/environment
fi

# --- REMOTE_URL ---
# Calculate REMOTE_URL if ENV var (from ARG) is empty
if [ -z "${REMOTE_URL}" ]; then
    # Try to get remote origin URL from git
    remote_url_raw=$(git config --get remote.origin.url 2>/dev/null || echo "")
    if [ -n "${remote_url_raw}" ]; then
        # Transform git URL (SSH or HTTPS) to web URL
        if [[ $remote_url_raw == "https://"* ]]; then
            # Already HTTPS, just remove .git suffix
            calculated_remote_url=$(echo "$remote_url_raw" | sed 's/\.git$//')
        else
            # Convert SSH URL to HTTPS URL
            calculated_remote_url=$(echo "$remote_url_raw" | sed 's/\.git$//' | sed 's/:/\//' | sed 's/^git@/https:\/\//')
        fi

        # Write calculated web URL if transformation was successful
        if [ -n "${calculated_remote_url}" ]; then
             echo "REMOTE_URL='${calculated_remote_url}'" >> /etc/environment
        fi
    fi
else
     # Ensure ARG-provided value is in /etc/environment (assume it's a valid web URL)
     echo "REMOTE_URL='${REMOTE_URL}'" >> /etc/environment
     # Use provided value for REMOTE_COMMIT_URL calculation below
     calculated_remote_url="${REMOTE_URL}"
fi

# --- Determine effective values for subsequent calculations ---
# Use ENV var value if set (from ARG), otherwise use calculated value
effective_commit_sha="${COMMIT_SHA:-$calculated_commit_sha}"
effective_short_commit_sha="${SHORT_COMMIT_SHA:-$calculated_short_commit_sha}"
effective_remote_url="${REMOTE_URL:-$calculated_remote_url}"

# --- REMOTE_COMMIT_URL ---
# Calculate and write REMOTE_COMMIT_URL if both components are available
if [ -z "${REMOTE_COMMIT_URL}" ] && [ -n "${effective_remote_url}" ] && [ -n "${effective_commit_sha}" ]; then
    echo "REMOTE_COMMIT_URL='${effective_remote_url}/commit/${effective_commit_sha}'" >> /etc/environment
else
    # Ensure ARG-provided value is in /etc/environment
    echo "REMOTE_COMMIT_URL='${REMOTE_COMMIT_URL}'" >> /etc/environment
fi

# --- CONDUWUIT_VERSION_EXTRA ---
# Calculate CONDUWUIT_VERSION_EXTRA if ENV var (from ARG) is empty
if [ -z "${CONDUWUIT_VERSION_EXTRA}" ]; then
    # Use the effective short commit sha, fallback to "unknown revision"
    calculated_version_extra="${effective_short_commit_sha:-unknown revision}"
    # Handle case where commit sha calculation failed and ARG wasn't set
    if [ -z "${calculated_version_extra}" ]; then
         calculated_version_extra="unknown revision"
    fi
    echo "CONDUWUIT_VERSION_EXTRA='${calculated_version_extra}'" >> /etc/environment
else
    # Ensure ARG-provided value is in /etc/environment
    echo "CONDUWUIT_VERSION_EXTRA='${CONDUWUIT_VERSION_EXTRA}'" >> /etc/environment
fi

EOF

ARG TARGETPLATFORM

# Verify environment configuration
RUN cat /etc/environment
RUN xx-cargo --print-target-triple


# Build the binary
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    --mount=type=cache,target=/app/target \
    bash <<'EOF'
    set -o allexport
    . /etc/environment
    TARGET_DIR=($(cargo metadata --no-deps --format-version 1 | \
            jq -r ".target_directory"))
    mkdir /out/sbin
    PACKAGE=conduwuit
    xx-cargo build --locked --release \
        -p $PACKAGE;
    BINARIES=($(cargo metadata --no-deps --format-version 1 | \
        jq -r ".packages[] | select(.name == \"$PACKAGE\") | .targets[] | select( .kind | map(. == \"bin\") | any ) | .name"))
    for BINARY in "${BINARIES[@]}"; do
        echo $BINARY
        xx-verify $TARGET_DIR/$(xx-cargo   --print-target-triple)/release/$BINARY
        cp $TARGET_DIR/$(xx-cargo --print-target-triple)/release/$BINARY /out/sbin/$BINARY
    done
EOF

# Generate Software Bill of Materials (SBOM)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    bash <<'EOF'
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
