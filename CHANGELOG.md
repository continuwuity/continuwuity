# Continuwuity 0.5.2 (2026-01-09)

## Features

- Added support for issuing additional registration tokens, stored in the database, which supplement the existing registration token hardcoded in the config file. These tokens may optionally expire after a certain number of uses or after a certain amount of time has passed. Additionally, the `registration_token_file` configuration option is superseded by this feature and **has been removed**. Use the new `!admin token` command family to manage registration tokens. Contributed by @ginger (#783).
- Implemented a configuration defined admin list independent of the admin room. Contributed by @Terryiscool160. (#1253)
- Added support for invite and join anti-spam via Draupnir and Meowlnir, similar to that of synapse-http-antispam. Contributed by @nex. (#1263)
- Implemented account locking functionality, to complement user suspension. Contributed by @nex. (#1266)
- Added admin command to forcefully log out all of a user's existing sessions. Contributed by @nex. (#1271)
- Implemented toggling the ability for an account to log in without mutating any of its data. Contributed by @nex. (#1272)
- Add support for custom room create event timestamps, to allow generating custom prefixes in hashed room IDs. Contributed by @nex. (#1277)
- Certain potentially dangerous admin commands are now restricted to only be usable in the admin room and server console. Contributed by @ginger.

## Bugfixes

- Fixed unreliable room summary fetching and improved error messages. Contributed by @nex. (#1257)
- Client requested timeout parameter is now applied to e2ee key lookups and claims. Related federation requests are now also concurrent. Contributed by @nex. (#1261)
- Fixed the whoami endpoint returning HTTP 404 instead of HTTP 403, which confused some appservices. Contributed by @nex. (#1276)

## Misc

- The `console` feature is now enabled by default, allowing the server console to be used for running admin commands directly. To automatically open the console on startup, set the `admin_console_automatic` config option to `true`. Contributed by @ginger.
- We now (finally) document our container image mirrors. Contributed by @Jade


# Continuwuity 0.5.0 (2025-12-30)

**This release contains a CRITICAL vulnerability patch, and you must update as soon as possible**

## Features

- Enabled the OTLP exporter in default builds, and allow configuring the exporter protocol. (@Jade). (#1251)

## Bug Fixes

- Don't allow admin room upgrades, as this can break the admin room (@timedout) (#1245)
- Fix invalid creators in power levels during upgrade to v12 (@timedout) (#1245)
