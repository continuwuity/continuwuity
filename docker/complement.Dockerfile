FROM ubuntu:latest
EXPOSE 8008
EXPOSE 8448
RUN apt-get update && apt-get install -y ca-certificates liburing2 && rm -rf /var/lib/apt/lists/*
RUN mkdir -p /etc/continuwuity /var/lib/continuwuity
COPY docker/complement-entrypoint.sh /usr/local/bin/complement-entrypoint.sh
COPY docker/complement.config.toml /etc/continuwuity/config.toml
COPY target/debug/conduwuit /usr/local/bin/conduwuit
RUN chmod +x /usr/local/bin/conduwuit /usr/local/bin/complement-entrypoint.sh
#HEALTHCHECK --interval=30s --timeout=5s CMD curl --fail http://localhost:8008/_continuwuity/server_version || exit 1
ENTRYPOINT ["/usr/local/bin/complement-entrypoint.sh"]
