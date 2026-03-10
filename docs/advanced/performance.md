# Performance tuning

While Continuwuity's default config parameters are generally optimized, additional modifications can be made to better utilize your server resources. This is especially helpful for homeservers with many users and/or are joined in many large federated rooms, and will increasingly be the case as the Matrix network expands.

This page aims to outline various performance tweaks for Continuwuity and their effects. As always, your mileage may vary according to your setup's specifics. If you have further discussions or recommendations, please share them in the community rooms.

## DNS tuning (recommended)

Please see the dedicated DNS tuning guide.

<!-- TODO: Write DNS tuning page and link it here -->

## Cache capacities

If you have unused memory to spare, consider increasing the `cache_capacity_modifier` value to a larger number, as to allow more data to be stored in hot memory. This would _significantly_ speed up many intensive operations such as state resolutions, and also results in decreased CPU usage and disk I/O. Start with a baseline of `cache_capacity_modifier = 2.0` and tune up until you find a satisfactory RAM usage.

On the other hand, if your system doesn't have a lot of RAM, consider decreasing the cache capacity modifier to something smaller than `1.0` to avoid low-memory issues (at the cost of higher load on disk/CPU). The recommendation also works if your system has very few RAM compared to the number of cores, as cache capacities tend to scale according to the latter.


## Disabling some features

You can disable outgoing **typing notifications** and **read markers** to reduce strain on the CPU and network.

```toml
# disables sending read receipts
allow_outgoing_read_receipts = false
# disables sending typing notifications
allow_incoming_typing = false
```

Outgoing presence updates is also considered expensive and is disabled by default(`allow_local_presence = false`).

For even more savings, you may wish to disable _all_ processing of typing notifications, read markers, and presence entirely. This can be done by also disabling the local and incoming events for these features.

<details>

<summary> `continuwuity.toml` </summary>

```toml
# disabling read receipts entirely
allow_local_read_receipts = false
allow_incoming_read_receipts = false
allow_outgoing_read_receipts = false

# disabling typing notifications entirely
allow_local_typing = false
allow_outgoing_typing = false
allow_incoming_typing = false

# disabling presence updates entirely
allow_local_presence = false
allow_incoming_presence = false
allow_outgoing_presence = false
```

</details>

## Tuning database compression

:::warning
These steps MUST be done **before** starting Continuwuity for the first time, as database compressions are irreversible
:::

### Changing the compression algorithm

For reduced CPU usage at a tradeoff of increased storage space, consider deploying Continuwuity with the faster and less intensive `lz4` algorithm instead of `zstd` for rocksdb, and disable WAL compression entirely:

```toml
### in continuwuity.toml ###
rocksdb_compression_algo = "lz4"
rocksdb_wal_compression = "none"
```

The tweak can especially be helpful if you have an older or less performant CPU (e.g. a Raspberry Pi) and disk space to spare.

### Increasing bottommost layer compression (`zstd` only)

The bottommost layer of the database usually contains old and read-only data, and hence is a suitable place for further compression. In Continuwuity, this is possible by setting `rocksdb_bottommost_compression = true` and tuning `rocksdb_bottommost_compression_level` to a more compact level than the default one used in `rocksdb_compression_level`. The tweak comes at a cost of some increased CPU usage, but would prevent your database from growing too large especially in the long run.

For those using `zstd` compression, the compression level ranges from 1 to 22. An example like this could apply:

```toml
### in continuwuity.toml ###
rocksdb_compression_algo = "zstd"
rocksdb_compression_level = 32767 # magic number, translates to level 3 on zstd
rocksdb_bottommost_compression = true
rocksdb_bottommost_compression_level = 9 # level 9 on zstd
```

For `lz4` users, the default level (`-1`) is already the most compact. You can only further decrease it to favor compression speed over ratio.

Consult these documentations for more information on compression tuning and levels:

- [Rocksdb compression documentation][rocksdb-compression]
- [Rocksdb default compression levels][rocksdb-compression-defaults]
- [Zstd manual][zstd-manual]
- [Lz4 manual][lz4-manual]

[rocksdb-compression]: https://github.com/facebook/rocksdb/wiki/Compression
[rocksdb-compression-defaults]: https://github.com/facebook/rocksdb/blob/main/include/rocksdb/options.h#L208-L217
[zstd-manual]: https://facebook.github.io/zstd/zstd_manual.html
[lz4-manual]: https://github.com/lz4/lz4/blob/release/doc/lz4_manual.html

## Other tweaks

You may consider exposing Continuwuity on a UNIX socket instead of a port if your reverse proxy is on the same machine, as this would reduce TCP overhead between them.

```toml
### in continuwuity.toml ###

# `address` and `port` has to be commented out first
#address = ["127.0.0.1", "::1"]
#port = 8008
unix_socket_path = "/run/continuwuity/continuwuity.sock"
```

```
### in your (example) Caddyfile ###
https://matrix.example.com {
    reverse_proxy unix//run/continuwuity/continuwuity.sock

    # alternatively, use the http2-plaintext protocol
    # reverse_proxy unix+h2c//run/continuwuity/continuwuity.sock
}
```
