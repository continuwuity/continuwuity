# Continuwuity for NixOS

Continuwuity can be acquired by Nix (or [Lix][lix]) from various places:

* The `flake.nix` at the root of the repo
* The `default.nix` at the root of the repo
* From Continuwuity's binary cache

### NixOS module

The `flake.nix` and `default.nix` do not currently provide a NixOS module (contributions
welcome!), so [`services.matrix-conduit`][module] from Nixpkgs can be used to configure
Continuwuity.

### Conduit NixOS Config Module and SQLite

Beware! The [`services.matrix-conduit`][module] module defaults to SQLite as a database backend.
Continuwuity dropped SQLite support in favor of exclusively supporting the much faster RocksDB.
Make sure that you are using the RocksDB backend before migrating!

There is a [tool to  migrate a Conduit SQLite database to
RocksDB](https://github.com/ShadowJonathan/conduit_toolbox/).

If you want to run the latest code, you should get Continuwuity from the `flake.nix`
or `default.nix` and set [`services.matrix-conduit.package`][package]
appropriately to use Continuwuity instead of Conduit.

### UNIX sockets

Due to the lack of a Continuwuity NixOS module, when using the `services.matrix-conduit` module
a workaround like the one below is necessary to use UNIX sockets. This is because the UNIX
socket option does not exist in Conduit, and the module forcibly sets the `address` and 
`port` config options.

```nix
options.services.matrix-conduit.settings = lib.mkOption {
  apply = old: old // (
    if (old.global ? "unix_socket_path")
    then { global = builtins.removeAttrs old.global [ "address" "port" ]; }
    else {  }
  );
};

```

Additionally, the [`matrix-conduit` systemd unit][systemd-unit] in the module does not allow
the `AF_UNIX` socket address family in their systemd unit's `RestrictAddressFamilies=` which
disallows the namespace from accessing or creating UNIX sockets and has to be enabled like so:

```nix
systemd.services.conduit.serviceConfig.RestrictAddressFamilies = [ "AF_UNIX" ];
```

Even though those workarounds are feasible a Continuwuity NixOS configuration module, developed and
published by the community, would be appreciated.

### jemalloc and hardened profile

Continuwuity uses jemalloc by default. This may interfere with the [`hardened.nix` profile][hardened.nix]
due to them using `scudo` by default. You must either disable/hide `scudo` from Continuwuity, or
disable jemalloc like so:

```nix
let
    conduwuit = pkgs.unstable.conduwuit.override {
      enableJemalloc = false;
    };
in
```

[lix]: https://lix.systems/
[module]: https://search.nixos.org/options?channel=unstable&query=services.matrix-conduit
[package]: https://search.nixos.org/options?channel=unstable&query=services.matrix-conduit.package
[hardened.nix]: https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/profiles/hardened.nix#L22
[systemd-unit]: https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/services/matrix/conduit.nix#L132
