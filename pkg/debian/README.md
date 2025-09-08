# Continuwuity for Debian

This document provides information about downloading and deploying the Debian package. You can also use this guide for other deb-based distributions such as Ubuntu.

### Installation

To add the Continuwuation apt repository:
```bash
# Import the Continuwuation signing key
sudo curl https://forgejo.ellis.link/api/packages/continuwuation/debian/repository.key -o /etc/apt/keyrings/forgejo-continuwuation.asc
# Add a new apt source list pointing to the repository
echo "deb [signed-by=/etc/apt/keyrings/forgejo-continuwuation.asc] https://forgejo.ellis.link/api/packages/continuwuation/debian $(lsb_release -sc) stable" | sudo tee -a /etc/apt/sources.list.d/continuwuation.list
# Update remote package lists
sudo apt update
```

To install continuwuity:
```bash
sudo apt install continuwuity
```
The `continuwuity` package conflicts with the old `conduwuit` package and will remove it automatically when installed.

See the [generic deployment guide](../deploying/generic.md) for additional information about using the Debian package.

### Configuration

After installation, Continuwuity places the example configuration at `/etc/conduwuit/conduwuit.toml` as the default configuration file. The configuration file indicates which settings you must change before starting the service.

You can customize additional settings by uncommenting and modifying the configuration options in `/etc/conduwuit/conduwuit.toml`.

### Running

The package uses the [`conduwuit.service`](../configuration/examples.md#example-systemd-unit-file) systemd unit file to start and stop Continuwuity. The binary installs at `/usr/bin/conduwuit`.

By default, this package assumes that Continuwuity runs behind a reverse proxy. The default configuration options apply (listening on `localhost` and TCP port `6167`). Matrix federation requires a valid domain name and TLS. To federate properly, you must set up TLS certificates and certificate renewal.

For information about setting up a reverse proxy and TLS, consult online documentation and guides. The [generic deployment guide](../deploying/generic.md#setting-up-the-reverse-proxy) documents Caddy, which is the most user-friendly option for reverse proxy configuration.
