# Generic deployment documentation

> ### Getting help
>
> If you run into any problems while setting up Continuwuity, ask us in
> `#continuwuity:continuwuity.org` or [open an issue on
> Forgejo](https://forgejo.ellis.link/continuwuation/continuwuity/issues/new).

## Installing Continuwuity

### Static prebuilt binary

You may simply download the binary that fits your machine architecture (x86_64
or aarch64). Run `uname -m` to see what you need.

You can download prebuilt fully static musl binaries from the latest tagged
release [here](https://forgejo.ellis.link/continuwuation/continuwuity/releases/latest) or
from the `main` CI branch workflow artifact output. These also include Debian/Ubuntu
packages.

You can download these directly using curl. The `ci-bins` are CI workflow binaries organized by commit
hash/revision, and `releases` are tagged releases. Sort by descending last
modified date to find the latest.

These binaries have jemalloc and io_uring statically linked and included with
them, so no additional dynamic dependencies need to be installed.

For the **best** performance: if you are using an `x86_64` CPU made in the last ~15 years,
we recommend using the `-haswell-` optimized binaries. These set
`-march=haswell`, which provides the most compatible and highest performance with
optimized binaries. The database backend, RocksDB, benefits most from this as it
uses hardware-accelerated CRC32 hashing/checksumming, which is critical
for performance.

### Compiling

Alternatively, you may compile the binary yourself.

### Building with the Rust toolchain

If wanting to build using standard Rust toolchains, make sure you install:

- (On linux) `liburing-dev` on the compiling machine, and `liburing` on the target host
- (On linux) `pkg-config` on the compiling machine to allow finding `liburing`
- A C++ compiler and (on linux) `libclang` for RocksDB

You can build Continuwuity using `cargo build --release --all-features`.

### Building with Nix

If you prefer, you can use Nix (or [Lix](https://lix.systems)) to build Continuwuity. This provides improved reproducibility and makes it easy to set up a build environment and generate output. This approach also allows for easy cross-compilation.

You can run the `nix build -L .#static-x86_64-linux-musl-all-features` or
`nix build -L .#static-aarch64-linux-musl-all-features` commands based
on architecture to cross-compile the necessary static binary located at
`result/bin/conduwuit`. This is reproducible with the static binaries produced
in our CI.

## Adding a Continuwuity user

While Continuwuity can run as any user, it is better to use dedicated users for
different services. This also ensures that the file permissions
are set up correctly.

In Debian, you can use this command to create a Continuwuity user:

```bash
sudo adduser --system continuwuity --group --disabled-login --no-create-home
```

For distros without `adduser` (or where it's a symlink to `useradd`):

```bash
sudo useradd -r --shell /usr/bin/nologin --no-create-home continuwuity
```

## Forwarding ports in the firewall or the router

Matrix's default federation port is 8448, and clients must use port 443.
If you would like to use only port 443 or a different port, you will need to set up
delegation. Continuwuity has configuration options for delegation, or you can configure
your reverse proxy to manually serve the necessary JSON files for delegation
(see the `[global.well_known]` config section).

If Continuwuity runs behind a router or in a container and has a different public
IP address than the host system, you need to forward these public ports directly
or indirectly to the port mentioned in the configuration.

Note for NAT users: if you have trouble connecting to your server from inside
your network, check if your router supports "NAT
hairpinning" or "NAT loopback".

If your router does not support this feature, you need to research doing local
DNS overrides and force your Matrix DNS records to use your local IP internally.
This can be done at the host level using `/etc/hosts`. If you need this to be
on the network level, consider something like NextDNS or Pi-Hole.

## Setting up a systemd service

You can find two example systemd units for Continuwuity
[on the configuration page](../configuration/examples.md#debian-systemd-unit-file).
You may need to change the `ExecStart=` path to match where you placed the Continuwuity
binary if it is not in `/usr/bin/conduwuit`.

On systems where rsyslog is used alongside journald (i.e. Red Hat-based distros
and OpenSUSE), put `$EscapeControlCharactersOnReceive off` inside
`/etc/rsyslog.conf` to allow color in logs.

If you are using a different `database_path` than the systemd unit's
configured default `/var/lib/conduwuit`, you need to add your path to the
systemd unit's `ReadWritePaths=`. You can do this by either directly editing
`conduwuit.service` and reloading systemd, or by running `systemctl edit conduwuit.service`
and entering the following:

```
[Service]
ReadWritePaths=/path/to/custom/database/path
```

## Creating the Continuwuity configuration file

Now you need to create the Continuwuity configuration file in
`/etc/continuwuity/continuwuity.toml`. You can find an example configuration at
[conduwuit-example.toml](../configuration/examples.md).

**Please take a moment to read the config. You need to change at least the
server name.**

RocksDB is the only supported database backend.

## Setting the correct file permissions

If you are using a dedicated user for Continuwuity, you need to allow it to
read the configuration. To do this, run:

```bash
sudo chown -R root:root /etc/conduwuit
sudo chmod -R 755 /etc/conduwuit
```

If you use the default database path you also need to run this:

```bash
sudo mkdir -p /var/lib/conduwuit/
sudo chown -R continuwuity:continuwuity /var/lib/conduwuit/
sudo chmod 700 /var/lib/conduwuit/
```

## Setting up the Reverse Proxy

We recommend Caddy as a reverse proxy because it is trivial to use and handles TLS certificates, reverse proxy headers, etc. transparently with proper defaults.
For other software, please refer to their respective documentation or online guides.

### Caddy

After installing Caddy via your preferred method, create `/etc/caddy/conf.d/conduwuit_caddyfile`
and enter the following (substitute your actual server name):

```caddyfile
your.server.name, your.server.name:8448 {
    # TCP reverse_proxy
    reverse_proxy 127.0.0.1:6167
    # UNIX socket
    #reverse_proxy unix//run/conduwuit/conduwuit.sock
}
```

That's it! Just start and enable the service and you're set.

```bash
sudo systemctl enable --now caddy
```

### Other Reverse Proxies

As we prefer our users to use Caddy, we do not provide configuration files for other proxies.

You will need to reverse proxy everything under the following routes:
- `/_matrix/` - core Matrix C-S and S-S APIs
- `/_conduwuit/` and/or `/_continuwuity/` - ad-hoc Continuwuity routes such as `/local_user_count` and
`/server_version`

You can optionally reverse proxy the following individual routes:
- `/.well-known/matrix/client` and `/.well-known/matrix/server` if using
Continuwuity to perform delegation (see the `[global.well_known]` config section)
- `/.well-known/matrix/support` if using Continuwuity to send the homeserver admin
contact and support page (formerly known as MSC1929)
- `/` if you would like to see `hewwo from conduwuit woof!` at the root

See the following spec pages for more details on these files:
- [`/.well-known/matrix/server`](https://spec.matrix.org/latest/client-server-api/#getwell-knownmatrixserver)
- [`/.well-known/matrix/client`](https://spec.matrix.org/latest/client-server-api/#getwell-knownmatrixclient)
- [`/.well-known/matrix/support`](https://spec.matrix.org/latest/client-server-api/#getwell-knownmatrixsupport)

Examples of delegation:
- <https://puppygock.gay/.well-known/matrix/server>
- <https://puppygock.gay/.well-known/matrix/client>

For Apache and Nginx there are many examples available online.

Lighttpd is not supported as it appears to interfere with the `X-Matrix` Authorization
header, making federation non-functional. If you find a workaround, please share it so we can add it to this documentation.

If using Apache, you need to use `nocanon` in your `ProxyPass` directive to prevent httpd from interfering with the `X-Matrix` header (note that Apache is not ideal as a general reverse proxy, so we discourage using it if alternatives are available).

If using Nginx, you need to pass the request URI to Continuwuity using `$request_uri`, like this:
- `proxy_pass http://127.0.0.1:6167$request_uri;`
- `proxy_pass http://127.0.0.1:6167;`

Nginx users need to increase the `client_max_body_size` setting (default is 1M) to match the
`max_request_size` defined in conduwuit.toml.

## You're done

Now you can start Continuwuity with:

```bash
sudo systemctl start conduwuit
```

Set it to start automatically when your system boots with:

```bash
sudo systemctl enable conduwuit
```

## How do I know it works?

You can open [a Matrix client](https://matrix.org/ecosystem/clients), enter your
homeserver address, and try to register.

You can also use these commands as a quick health check (replace
`your.server.name`).

```bash
curl https://your.server.name/_conduwuit/server_version

# If using port 8448
curl https://your.server.name:8448/_conduwuit/server_version

# If federation is enabled
curl https://your.server.name:8448/_matrix/federation/v1/version
```

- To check if your server can communicate with other homeservers, use the
[Matrix Federation Tester](https://federationtester.matrix.org/). If you can
register but cannot join federated rooms, check your configuration and verify
that port 8448 is open and forwarded correctly.

# What's next?

## Audio/Video calls

For Audio/Video call functionality see the [TURN Guide](../turn.md).

## Appservices

If you want to set up an appservice, take a look at the [Appservice
Guide](../appservices.md).
