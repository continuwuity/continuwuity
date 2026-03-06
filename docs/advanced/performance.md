# Performance tuning

While Continuwuity's default config parameters are optimized for a small instance, they would likely need additional modifications to smoothly run in a larger context. This is especially true for homeservers with many users and/or are joined in many large federated rooms, and will increasingly be the case as the Matrix network expands.

This page aims to outline various performance tweaks for Continuwuity and their effects. As always, your mileage may vary according to your setup's specifics. If you have further discussions or recommendations, please share them in the community rooms.

## DNS tuning (recommended)

Please see the dedicated DNS tuning guide.

<!-- TODO: Write DNS tuning page and link it here -->

## Cache capacities

If you have unused memory to spare, consider increasing the `cache_capacity_modifier` value to a larger number, as to allow more data to be stored in hot memory. This would _significantly_ speed up many intensive operations such as state resolutions, and also result in decreased CPU usage and disk I/O. Start with a baseline of `cache_capacity_modifier = 2.0` and tune up until you find a satisfactory RAM usage.

On the other hand, if your system doesn't have a lot of RAM, consider decreasing the cache capacity modifier to something smaller than `1.0` to avoid low-memory issues (at the cost of higher load on disk/CPU). The recommendation also works if your system has very few RAM compared to the number of cores, as cache capacities tend to scale according to the latter.

## Changing database compression algorithm

:::warning
This step should be done **before** starting Continuwuity for the first time
:::

For reduced CPU usage at a tradeoff of increased storage space, consider deploying Continuwuity with the less intensive `lz4` algorithm instead of `zstd` for rocksdb, and disable WAL compression entirely:

```toml
### in continuwuity.toml ###
rocksdb_compression_algo = "lz4"
rocksdb_wal_compression = "none"
```

The tweak can especially be helpful if you have an older or less performant CPU (e.g. a Raspberry Pi) and disk space to spare.

## Disabling some features

You can consider disabling **typing notifications** and **read markers** to reduce strain on the CPU and network, especially for outbound requests.

```toml
# disabling read receipts
allow_local_read_receipts = false
allow_incoming_read_receipts = false
allow_outgoing_read_receipts = false

# disabling typing notifications
allow_local_typing = false
allow_outgoing_typing = false
allow_incoming_typing = false
```

Presence is also considered expensive and is disabled by default. For reference, you can also disable them manually as follows:

```toml
allow_local_presence = false
allow_incoming_presence = false
allow_outgoing_presence = false
```

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
