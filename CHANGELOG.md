# Continuwuity 0.5.6 (2026-03-03)

## Security

- Admin escape commands received over federation will never be executed, as this is never valid in a genuine situation. Contributed by @Jade.
- Fixed data amplification vulnerability (CWE-409) that affected configurations with server-side compression enabled (non-default). Contributed by @nex.

## Features

- Outgoing presence is now disabled by default, and the config option documentation has been adjusted to more accurately represent the weight of presence, typing indicators, and read receipts. Contributed by @nex. ([#1399](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1399))
- Improved the concurrency handling of federation transactions, vastly improving performance and reliability by more accurately handling inbound transactions and reducing the amount of repeated wasted work. Contributed by @nex and @Jade. ([#1428](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1428))
- Added [MSC3202](https://github.com/matrix-org/matrix-spec-proposals/pull/3202) Device masquerading (not all of MSC3202). This should fix issues with enabling [MSC4190](https://github.com/matrix-org/matrix-spec-proposals/pull/4190) for some Mautrix bridges. Contributed by @Jade ([#1435](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1435))
- Added [MSC3814](https://github.com/matrix-org/matrix-spec-proposals/pull/3814) Dehydrated Devices - you can now decrypt messages sent while all devices were logged out. ([#1436](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1436))
- Implement [MSC4143](https://github.com/matrix-org/matrix-spec-proposals/pull/4143) MatrixRTC transport discovery endpoint. Move RTC foci configuration from `[global.well_known]` to a new `[global.matrix_rtc]` section with a `foci` field. Contributed by @0xnim ([#1442](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1442))
- Updated `list-backups` admin command to output one backup per line. ([#1394](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1394))
- Improved URL preview fetching with a more compatible user agent for sites like YouTube Music. Added `!admin media delete-url-preview <url>` command to clear cached URL previews that were stuck and broken. ([#1434](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1434))

## Bugfixes

- Removed non-compliant nor functional room alias lookups over federation. Contributed by @nex ([#1393](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1393))
- Removed ability to set rocksdb as read only. Doing so would cause unintentional and buggy behaviour. Contributed by @Terryiscool160. ([#1418](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1418))
- Fixed a startup crash in the sender service if we can't detect the number of CPU cores, even if the `sender_workers` config option is set correctly. Contributed by @katie. ([#1421](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1421))
- Removed the `allow_public_room_directory_without_auth` config option. Contributed by @0xnim. ([#1441](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1441))
- Fixed sliding sync v5 list ranges always starting from 0, causing extra rooms to be unnecessarily processed and returned. Contributed by @0xnim ([#1445](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1445))
- Fixed a bug that (repairably) caused a room split between continuwuity and non-continuwuity servers when the room had both `m.room.policy` and `org.matrix.msc4284.policy` in its room state. Contributed by @nex ([#1481](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1481))
- Fixed `!admin media delete --mxc <url>` responding with an error message when the media was deleted successfully. Contributed by @lynxize
- Fixed spurious 404 media errors in the logs. Contributed by @benbot.
- Fixed spurious warn about needed backfill via federation for non-federated rooms. Contributed by @kraem.

# Continuwuity v0.5.5 (2026-02-15)

## Features

- Added unstable support for [MSC4406:
  `M_SENDER_IGNORED`](https://github.com/matrix-org/matrix-spec-proposals/pull/4406).
  Contributed by @nex ([#1308](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1308))
- Introduce a resolver command to allow flushing a server from the cache or to flush the complete cache. Contributed by
  @Omar007 ([#1349](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1349))
- Improved the handling of restricted join rules and improved the performance of local-first joins. Contributed by
  @nex. ([#1368](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1368))
- You can now set a custom User Agent for URL previews; the default one has been modified to be less likely to be
  rejected. Contributed by @trashpanda ([#1372](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1372))
- Improved the first-time setup experience for new homeserver administrators:
    - Account registration is disabled on the first run, except for with a new special registration token that is logged
      to the console.
    - Other helpful information is logged to the console as well, including a giant warning if open registration is
      enabled.
    - The default index page now says to check the console for setup instructions if no accounts have been created.
    - Once the first admin account is created, an improved welcome message is sent to the admin room.

  Contributed by @ginger.

## Bugfixes

- Fixed invites sent to other users in the same homeserver not being properly sent down sync. Users with missing or
  broken invites should clear their client caches after updating to make them appear. ([#1249](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1249))
- LDAP-enabled servers will no longer have all admins demoted when LDAP-controlled admins are not configured.
  Contributed by @Jade ([#1307](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1307))
- Fixed sliding sync not resolving wildcard state key requests, enabling Video/Audio calls in Element X. ([#1370](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1370))

## Misc

- #1344

# Continuwuity v0.5.4 (2026-02-08)

## Features

- The announcement checker will now announce errors it encounters in the first run to the admin room, plus a few other
  misc improvements. Contributed by @Jade ([#1288](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1288))
- Drastically improved the performance and reliability of account deactivations. Contributed by
  @nex ([#1314](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1314))
- Refuse to process requests for and events in rooms that we no longer have any local users in (reduces state resets
  and improves performance). Contributed by
  @nex ([#1316](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1316))
- Added server-specific admin API routes to ban and unban rooms, for use with moderation bots. Contributed by @nex
  ([#1301](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1301))

## Bugfixes

- Fix the generated configuration containing uncommented optional sections. Contributed by
  @Jade ([#1290](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1290))
- Fixed specification non-compliance when handling remote media errors. Contributed by
  @nex ([#1298](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1298))
- UIAA requests which check for out-of-band success (sent by matrix-js-sdk) will no longer create unhelpful errors in
  the logs. Contributed by @ginger ([#1305](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1305))
- Use exists instead of contains to save writing to a buffer in `src/service/users/mod.rs`: `is_login_disabled`.
  Contributed
  by @aprilgrimoire. ([#1340](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1340))
- Fixed backtraces being swallowed during panics. Contributed by
  @jade ([#1337](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1337))
- Fixed a potential vulnerability that could allow an evil remote server to return malicious events during the room join
  and knock process. Contributed by @nex, reported by violet & [mat](https://matdoes.dev).
- Fixed a race condition that could result in outlier PDUs being incorrectly marked as visible to a remote server.
  Contributed by @nex, reported by violet & [mat](https://matdoes.dev).
- ACLs are no longer case-sensitive. Contributed by @nex, reported by [vel](matrix:u/vel:nhjkl.com?action=chat).

## Docs

- Fixed Fedora install instructions. Contributed by
  @julian45 ([#1342](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1342))

# Continuwuity 0.5.3 (2026-01-12)

## Features

- Improve the display of nested configuration with the `!admin server show-config` command. Contributed by
  @Jade ([#1279](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1279))

## Bugfixes

- Fixed `M_BAD_JSON` error when sending invites to other servers or when providing joins. Contributed by
  @nex ([#1286](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1286))

## Docs

- Improve admin command documentation generation. Contributed by
  @ginger ([#1280](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1280))

## Misc

- Improve timeout-related code for federation and URL previews. Contributed by
  @Jade ([#1278](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1278))

# Continuwuity 0.5.2 (2026-01-09)

## Features

- Added support for issuing additional registration tokens, stored in the database, which supplement the existing
  registration token hardcoded in the config file. These tokens may optionally expire after a certain number of uses or
  after a certain amount of time has passed. Additionally, the `registration_token_file` configuration option is
  superseded by this feature and **has been removed**. Use the new `!admin token` command family to manage registration
  tokens. Contributed by @ginger (#783).
- Implemented a configuration defined admin list independent of the admin room. Contributed by
  @Terryiscool160. ([#1253](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1253))
- Added support for invite and join anti-spam via Draupnir and Meowlnir, similar to that of synapse-http-antispam.
  Contributed by @nex. ([#1263](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1263))
- Implemented account locking functionality, to complement user suspension. Contributed by
  @nex. ([#1266](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1266))
- Added admin command to forcefully log out all of a user's existing sessions. Contributed by
  @nex. ([#1271](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1271))
- Implemented toggling the ability for an account to log in without mutating any of its data. Contributed by @nex. (
  [#1272](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1272))
- Add support for custom room create event timestamps, to allow generating custom prefixes in hashed room IDs.
  Contributed by @nex. ([#1277](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1277))
- Certain potentially dangerous admin commands are now restricted to only be usable in the admin room and server
  console. Contributed by @ginger.

## Bugfixes

- Fixed unreliable room summary fetching and improved error messages. Contributed by
  @nex. ([#1257](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1257))
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

- Enabled the OTLP exporter in default builds, and allow configuring the exporter protocol. (
  @Jade). ([#1251](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1251))

## Bug Fixes

- Don't allow admin room upgrades, as this can break the admin room (
  @timedout) ([#1245](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1245))
- Fix invalid creators in power levels during upgrade to v12 (
  @timedout) ([#1245](https://forgejo.ellis.link/continuwuation/continuwuity/pulls/1245))
