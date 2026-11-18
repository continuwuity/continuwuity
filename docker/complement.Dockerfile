ARG CONTINUWUITY_IMAGE=continuwuity:latest-openreg
ARG BINARY_SOURCE=host

FROM ${CONTINUWUITY_IMAGE} AS container-binary

FROM scratch AS host-binary
ARG BINARY_PATH=target/debug/conduwuit
COPY ${BINARY_PATH} /sbin/conduwuit

FROM ubuntu:latest AS base
EXPOSE 8008
EXPOSE 8448
RUN apt-get update && apt-get install -y ca-certificates liburing2 && rm -rf /var/lib/apt/lists/*
RUN mkdir -p /etc/continuwuity /var/lib/continuwuity /usr/local/bin/
COPY complement/complement-entrypoint.sh /usr/local/bin/complement-entrypoint.sh
COPY complement/complement.config.toml /etc/continuwuity/config.toml
RUN chmod +x /usr/local/bin/complement-entrypoint.sh
#HEALTHCHECK --interval=30s --timeout=5s CMD curl --fail http://localhost:8008/_continuwuity/server_version || exit 1
ENTRYPOINT ["/usr/local/bin/complement-entrypoint.sh"]

FROM base AS container
COPY --from=container-binary /sbin/conduwuit /usr/local/bin/conduwuit
RUN chmod +x /usr/local/bin/conduwuit

FROM base AS host
COPY --from=host-binary /sbin/conduwuit /usr/local/bin/conduwuit
RUN chmod +x /usr/local/bin/conduwuit

FROM ${BINARY_SOURCE}
