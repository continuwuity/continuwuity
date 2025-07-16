# Continuwuity for NixOS

NixOS packages Continuwuity as `matrix-continuwuity`. This package includes both the Continuwuity software and a dedicated NixOS module for configuration and deployment.

## Installation methods

You can acquire Continuwuity with Nix (or [Lix][lix]) from these sources:

* Directly from Nixpkgs using the official package (`pkgs.matrix-continuwuity`)
* The `flake.nix` at the root of the Continuwuity repo
* The `default.nix` at the root of the Continuwuity repo

## NixOS module

Continuwuity now has an official NixOS module that simplifies configuration and deployment. The module is available in Nixpkgs as `services.matrix-continuwuity` from NixOS 25.05.

Here's a basic example of how to use the module:

```nix
{ config, pkgs, ... }:

{
  services.matrix-continuwuity = {
    enable = true;
    settings = {
      global = {
        server_name = "example.com";
        # Listening on localhost by default
        # address and port are handled automatically
        allow_registration = false;
        allow_encryption = true;
        allow_federation = true;
        trusted_servers = [ "matrix.org" ];
      };
    };
  };
}
```

### Available options

The NixOS module provides these configuration options:

- `enable`: Enable the Continuwuity service
- `user`: The user to run Continuwuity as (defaults to "continuwuity")
- `group`: The group to run Continuwuity as (defaults to "continuwuity")
- `extraEnvironment`: Extra environment variables to pass to the Continuwuity server
- `package`: The Continuwuity package to use
- `settings`: The Continuwuity configuration (in TOML format)

Use the `settings` option to configure Continuwuity itself. See the [example configuration file](../configuration/examples.md#example-configuration) for all available options.

### UNIX sockets

The NixOS module natively supports UNIX sockets through the `global.unix_socket_path` option. When using UNIX sockets, set `global.address` to `null`:

```nix
services.matrix-continuwuity = {
  enable = true;
  settings = {
    global = {
      server_name = "example.com";
      address = null; # Must be null when using unix_socket_path
      unix_socket_path = "/run/continuwuity/continuwuity.sock";
      unix_socket_perms = 660; # Default permissions for the socket
      # ...
    };
  };
};
```

The module automatically sets the correct `RestrictAddressFamilies` in the systemd service configuration to allow access to UNIX sockets.

### RocksDB database

Continuwuity exclusively uses RocksDB as its database backend. The system configures the database path automatically to `/var/lib/continuwuity/` and you cannot change it due to the service's reliance on systemd's StateDir.

If you're migrating from Conduit with SQLite, use this [tool to migrate a Conduit SQLite database to RocksDB](https://github.com/ShadowJonathan/conduit_toolbox/).

### jemalloc and hardened profile

Continuwuity uses jemalloc by default. This may interfere with the [`hardened.nix` profile][hardened.nix] because it uses `scudo` by default. Either disable/hide `scudo` from Continuwuity or disable jemalloc like this:

```nix
services.matrix-continuwuity = {
  enable = true;
  package = pkgs.matrix-continuwuity.override {
    enableJemalloc = false;
  };
  # ...
};
```

## Upgrading from Conduit

If you previously used Conduit with the `services.matrix-conduit` module:

1. Ensure your Conduit uses the RocksDB backend, or migrate from SQLite using the [migration tool](https://github.com/ShadowJonathan/conduit_toolbox/)
2. Switch to the new module by changing `services.matrix-conduit` to `services.matrix-continuwuity` in your configuration
3. Update any custom configuration to match the new module's structure

## Reverse proxy configuration

You'll need to set up a reverse proxy (like nginx or caddy) to expose Continuwuity to the internet. Configure your reverse proxy to forward requests to `/_matrix` on port 443 and 8448 to your Continuwuity instance.

Here's an example nginx configuration:

```nginx
server {
    listen 443 ssl;
    listen [::]:443 ssl;
    listen 8448 ssl;
    listen [::]:8448 ssl;

    server_name example.com;

    # SSL configuration here...

    location /_matrix/ {
        proxy_pass http://127.0.0.1:6167$request_uri;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

[lix]: https://lix.systems/
[hardened.nix]: https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/profiles/hardened.nix
