# Continuwuity v0.5.4 (2026-02-08)

## Features

- The announcement checker will now announce errors it encounters in the first run to the admin room, plus a few other
  misc improvements. Contributed by @Jade ([#1288](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1288))
- Drastically improved the performance and reliability of account deactivations. Contributed by @nex ([#1314](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1314))
- Refuse to process requests for and events in rooms that we no longer have any local users in (reduces state resets
  and improves performance). Contributed by @nex ([#1316](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1316))
- Added server-specific admin API routes to ban and unban rooms, for use with moderation bots. Contributed by @nex
  ([#1301](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1301))

## Bugfixes

- Fix the generated configuration containing uncommented optional sections. Contributed by @Jade ([#1290](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1290))
- Fixed specification non-compliance when handling remote media errors. Contributed by @nex ([#1298](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1298))
- UIAA requests which check for out-of-band success (sent by matrix-js-sdk) will no longer create unhelpful errors in
  the logs. Contributed by @ginger ([#1305](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1305))
- Use exists instead of contains to save writing to a buffer in `src/service/users/mod.rs`: `is_login_disabled`.
  Contributed
  by @aprilgrimoire. ([#1340](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1340))
- Fixed backtraces being swallowed during panics. Contributed by @jade ([#1337](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1337))
- Fixed a potential vulnerability that could allow an evil remote server to return malicious events during the room join
  and knock process. Contributed by @nex, reported by violet & [mat](https://matdoes.dev).
- Fixed a race condition that could result in outlier PDUs being incorrectly marked as visible to a remote server.
  Contributed by @nex, reported by violet & [mat](https://matdoes.dev).
- ACLs are no longer case-sensitive. Contributed by @nex, reported by [vel](matrix:u/vel:nhjkl.com?action=chat).

## Docs

- Fixed Fedora install instructions. Contributed by @julian45 ([#1342](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1342))

# Continuwuity 0.5.3 (2026-01-12)

## Features

- Improve the display of nested configuration with the `!admin server show-config` command. Contributed by @Jade ([#1279](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1279))

## Bugfixes

- Fixed `M_BAD_JSON` error when sending invites to other servers or when providing joins. Contributed by @nex ([#1286](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1286))

## Docs

- Improve admin command documentation generation. Contributed by @ginger ([#1280](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1280))

## Misc

- Improve timeout-related code for federation and URL previews. Contributed by @Jade ([#1278](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1278))

# Continuwuity 0.5.2 (2026-01-09)

## Features

- Added support for issuing additional registration tokens, stored in the database, which supplement the existing
  registration token hardcoded in the config file. These tokens may optionally expire after a certain number of uses or
  after a certain amount of time has passed. Additionally, the `registration_token_file` configuration option is
  superseded by this feature and **has been removed**. Use the new `!admin token` command family to manage registration
  tokens. Contributed by @ginger (#783).
- Implemented a configuration defined admin list independent of the admin room. Contributed by @Terryiscool160. ([#1253](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1253))
- Added support for invite and join anti-spam via Draupnir and Meowlnir, similar to that of synapse-http-antispam.
  Contributed by @nex. ([#1263](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1263))
- Implemented account locking functionality, to complement user suspension. Contributed by @nex. ([#1266](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1266))
- Added admin command to forcefully log out all of a user's existing sessions. Contributed by @nex. ([#1271](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1271))
- Implemented toggling the ability for an account to log in without mutating any of its data. Contributed by @nex. (
  [#1272](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1272))
- Add support for custom room create event timestamps, to allow generating custom prefixes in hashed room IDs.
  Contributed by @nex. ([#1277](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1277))
- Certain potentially dangerous admin commands are now restricted to only be usable in the admin room and server
  console. Contributed by @ginger.

## Bugfixes

- Fixed unreliable room summary fetching and improved error messages. Contributed by @nex. ([#1257](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1257))
- Client requested timeout parameter is now applied to e2ee key lookups and claims. Related federation requests are now
  also concurrent. Contributed by @nex. ([#1261](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1261))
- Fixed the whoami endpoint returning HTTP 404 instead of HTTP 403, which confused some appservices. Contributed by
  @nex. ([#1276](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1276))

## Misc

- The `console` feature is now enabled by default, allowing the server console to be used for running admin commands
  directly. To automatically open the console on startup, set the `admin_console_automatic` config option to `true`.
  Contributed by @ginger.
- We now (finally) document our container image mirrors. Contributed by @Jade

# Continuwuity 0.5.0 (2025-12-30)

**This release contains a CRITICAL vulnerability patch, and you must update as soon as possible**

## Features

- Enabled the OTLP exporter in default builds, and allow configuring the exporter protocol. (@Jade). ([#1251](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1251))

## Bug Fixes

- Don't allow admin room upgrades, as this can break the admin room (@timedout) ([#1245](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1245))
- Fix invalid creators in power levels during upgrade to v12 (@timedout) ([#1245](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1245))
