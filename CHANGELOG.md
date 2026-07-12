# Continuwuity 26.6.2 (2026-07-12)

## Bugfixes

- Fixed the server returning 500 errors if `admin_console_automatic` is enabled and no TTY is available. Contributed by @s1lv3r. (#1975)
- Fixed `global.oauth.compatibility_mode` being required, despite being ignored, when the `[global.oauth.oidc]` config section is provided.
- Fixed an issue with a migration that could cause user accounts imported from an identity provider to be marked as deactivated when the server started. If you have accounts affected by this issue, use `!admin users reset-password --convert-to-local-account` to reactivate them.


# Continuwuity 26.6.1 (2026-07-12)

## Features

- Added enforcement for new federated invite checks and corrected a bunch of related spec compliance issues along the way. Contributed by @nex. (#1952)

## Bugfixes

- Fixed existing accounts failing to link when logging in with OIDC if `prompt_for_localpart` was `false`. (#1942)
- Authentication is no longer required on the `/_matrix/client/v3/account/3pid/email/requestToken` endpoint. (#1953)
- Fixed newly created rooms failing to sync properly in clients using legacy sync.
- Stopped appservice users from being erroneously marked as deactivated during a 26.6 database migration.
- Whitespace will now automatically be trimmed from the start and end of the `global.oauth.oidc.client_secret_file`.


# Continuwuity 26.6.0 (2026-07-10)

## Features

- Added support for linking an external identity provider with OIDC. Contributed by @ginger. (#765)
- Updated [MSC4284: Policy Servers](https://github.com/matrix-org/matrix-spec-proposals/pull/4284) implementation to support the newly stabilised proposal. Contributed by @nex. (#1487)
- Added config option for default room ACLs. Contributed by @eve. (#1691)
- Added support for fallback encryption keys. (#1710)
- Add `!admin users reject-all-invites` to clean invite spam (#1741)
- Implemented event rejection, which should resolve and prevent future netsplits of the kinds observed
  within some Continuwuity rooms.
  Also resolved several bugs related to both soft-failing events, and event backfilling, which should
  improve state resolution stability.
  The `!admin debug get-pdu` command was updated to disambiguate event acceptance status, and
  `!admin debug show-auth-chain` was added to visually display event auth chains, which may assist
  developers in debugging strangely complex events.

  Contributed by @nex. (#1747)
- Added full support for [MSC4168: Update `m.space.*` state on room upgrade](https://github.com/matrix-org/matrix-spec-proposals/pull/4168). Contributed by @nex. (#1807)
- Improved the performance and reliability of fetching missing events, improving network partition recovery. Contributed
  by @nex. (#1818)
- Added static builds using Nix, allowing for Continuwuity on musl. During this, we also introduced a `max-perf-haswell` package, separating it from `max-perf`, so you may want to swap to this if you are on NixOS. Contributed by @Henry-Hiles (QuadRadical). (#1853)
- Added support for MSC4380 invite blocking, which has become part of the Matrix specification in v1.18. Contributed by @nex. (#1875)
- Added `!admin debug get-state-at` command (#1877)
- Added a configuration option to allow choosing a client IP source that is not the TCP connecting IP. Contributed by @nex. (#1931)
- Added support for MSC4466, which allows clients to customize how changes to a user's global profile are propagated. Contributed by @ginger.
- Added support for Matrix 1.16's `state_after` feature, allowing clients which understand it to sync room state changes more reliably. Contributed by @ginger.
- Added support for authenticating clients using the new OAuth 2.0 login API. Contributed by @ginger.
- Appservice device management as outlined in MSC4190 (part of Matrix 1.17) is now fully supported. Contributed by @ginger.
- Users may now be forbidden from deactivating their own accounts with the new `allow_deactivation` config option. Contributed by @ginger.

## Bugfixes

- Adjusted legacy sync logic to allow the `roomsynctoken_shortstatehash` database column to be dropped, massively reducing database sizes, especially for old deployments. Contributed by @ginger. (#917)
- Fixed a bug that caused the server to drop events during processing if several events for the same room were sent in a singular transaction. Contributed by @nex. (#1711)
- fix `!admin query account-data account-data-get` not returning the content (#1742)
- Fixed an issue where Continuwuity would only advertise support for the unstable endpoint for Mutual Rooms (MSC2666), despite only supporting the stable endpoint. Contributed by @Henry-Hiles (QuadRadical) (#1752)
- Fixed admin commands being ignored when they had leading whitespace before admin commands. Contributed by @kitvonsnookerz. (#1804)
- Fixed several bugs in the `POST /_matrix/client/v3/rooms/{roomId}/upgrade` endpoint. Contributed by @nex. (#1807)
- Devices which set their presence as "offline" will no longer be considered for presence updates. Contributed by @timedout.
- Improved invite and join reliability in clients using legacy sync. Contributed by @ginger
- The invite recipient's membership event is now included in invite stripped state, which should fix flaky invite display in some clients. Contributed by @ginger

## Improved Documentation

- Add performance tuning documentation. Contributed by @stratself. (#1498)
- Explain accessing Continuwuity's server console when deployed via Docker. (#1671)
- Clarified in the config that `max_request_size` affects federated media as well. (#1706)
- Added example configuration using caddy-docker-proxy in the livekit setup section of the docs. Contributed by @Cease (#1762)
- Updated deployment docs to account for new RPM package availability across more distros. Contributed by @julian45. (#1912)

## Deprecations and Removals

- Removed support for LDAP. (#1701)
- Removed support for guest user registration, a little-used and deprecated approach to room previews.
- Removed the `/_conduwuit/` versions of the `local_user_count` and `version` routes. These routes are still accessible under the `/_continuwuity` prefix.
- Support for server-side blurhashing (part of MSC2448) has been removed.
- The deprecated `well_known.rtc_focus_server_urls` config option has been removed. MatrixRTC foci should be configured using the `matrix_rtc.foci` config option.

## Misc

- #1505, #1829, #1927, #1933, #1934
- Switched from Continuwuity's fork of Ruma back to upstream Ruma. Contributed by @ginger.
- The version of Debian that the Docker-based build process uses has been upgraded from Bookworm to Trixie, meaning that standalone binaries now have a minimum glibc of 2.41, and can no longer be used on distro versions from before 2025-01-30


# Continuwuity 0.5.8 (2026-04-24)

## Features

- LDAP can now optionally be connected to using StartTLS, and you may unsafely skip verification. Contributed by @getz (#1389)
- Users will now be prevented from removing their email if the server is configured to require an email when registering an account.

## Bugfixes

- Fixed a situation where multiple email addresses could be associated with one user when that user changes their email address.

## Improved Documentation

- Updated config docs to state we support room version 12, and set it as default. Contributed by @ezera. (#1622)
- Improve instructions for generic deployments, removing unnecessary parts and documenting the new initial registration token flow. Contributed by @stratself (#1677)


# Continuwuity v0.5.7 (2026-04-17)

## Features

- Re-added support for reading registration tokens from a file. Contributed by @ginger and @benbot. (#1371)
- Add new config option to allow or disallow search engine indexing through a `<meta ../>` tag. Defaults to blocking indexing (`content="noindex"`). Contributed by @s1lv3r and @ginger. (#1527)
- Add new config option for [MSC4439](https://github.com/matrix-org/matrix-spec-proposals/pull/4439)
  PGP key URIs. Contributed by LogN. (#1609)
- Added `!admin users reset-push-rules` command to reset the notification settings of users. Contributed by @nex. (#1613)
- Notification pushers are now automatically removed when their associated device is. Admin commands now exist for manual cleanup too. Contributed by @nex. (#1614)
- Implemented option to deprioritize servers for room join requests. Contributed by @ezera. (#1624)
- Added admin commands to get build information and features. Contributed by @Jade (#1629)
- Added support for associating email addresses with accounts, requiring email addresses for registration, and resetting passwords via email. Contributed by @ginger
- Added support for requiring users to accept terms and conditions when registering.
- Added support for using an admin command to issue self-service password reset links.

## Bugfixes

- Fixed corrupted appservice registrations causing the server to enter a crash loop. Contributed by @nex. (#1265)
- Prevent removing the admin room alias (`#admins`) to avoid accidentally breaking admin room functionality. Contributed by @0xnim (#1448)
- Stripped `join_authorised_via_users_server` from json if user is already in room (@partha:cxy.run) (#1542)
- Fixed internal server errors for fetching thumbnails. Contributed by @PerformativeJade (#1572)
- Fixed error 500 when joining non-existent rooms. Contributed by @ezera. (#1579)
- Refactored nix package. Breaking, since `all-features` package no longer exists. Continuwuity is now built with jemalloc and liburing by default. Contributed by @Henry-Hiles (QuadRadical). (#1596)
- Fixed resolving IP of servers that only use SRV delegation. Contributed by @tulir. (#1615)
- Fixed "Sender must be a local user" error for make_join, make_knock, and make_leave federation routes. Contributed by @nex. (#1623)
- Fixed restricted joins not being signed when we are being used as an authorising server. Contributed by @nex, reported by [vel](matrix:u/vel:nhjkl.com?action=chat). (#1630)
- Fixed room alias deletion so removing one local alias no longer removes other aliases from room alias listings.
- Stopped left rooms from being unconditionally sent on initial sync, hopefully fixing spurious appearances of left rooms in some clients (and making sync faster as a bonus). Contributed by @ginger
- Correct the response field name for MatrixRTC transports. Contributed by @spaetz

## Improved Documentation

- Added Testing and Troubleshooting instructions for Livekit documentation. Contributed by @stratself. (#1429)
- Refactored docker docs to include new initial token workflow, and add Caddyfile example. Contributed by @stratself. (#1594)
- Add DNS tuning guide for Continuwuity. Users are recommended to set up a local caching resolver following the guide's advice. Contributed by @stratself (#1601)

## Misc

- Fixed compiler warning in cf_opts.rs when building in release. Contributed by @ezera. (#1620)


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
