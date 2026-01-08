# Command-Line Help for `admin`

This document contains the help content for the `admin` command-line program.

**Command Overview:**

* [`admin`↴](#admin)
* [`admin appservices`↴](#admin-appservices)
* [`admin appservices register`↴](#admin-appservices-register)
* [`admin appservices unregister`↴](#admin-appservices-unregister)
* [`admin appservices show-appservice-config`↴](#admin-appservices-show-appservice-config)
* [`admin appservices list-registered`↴](#admin-appservices-list-registered)
* [`admin users`↴](#admin-users)
* [`admin users create-user`↴](#admin-users-create-user)
* [`admin users reset-password`↴](#admin-users-reset-password)
* [`admin users deactivate`↴](#admin-users-deactivate)
* [`admin users deactivate-all`↴](#admin-users-deactivate-all)
* [`admin users logout`↴](#admin-users-logout)
* [`admin users suspend`↴](#admin-users-suspend)
* [`admin users unsuspend`↴](#admin-users-unsuspend)
* [`admin users lock`↴](#admin-users-lock)
* [`admin users unlock`↴](#admin-users-unlock)
* [`admin users enable-login`↴](#admin-users-enable-login)
* [`admin users disable-login`↴](#admin-users-disable-login)
* [`admin users list-users`↴](#admin-users-list-users)
* [`admin users list-joined-rooms`↴](#admin-users-list-joined-rooms)
* [`admin users force-join-room`↴](#admin-users-force-join-room)
* [`admin users force-leave-room`↴](#admin-users-force-leave-room)
* [`admin users force-leave-remote-room`↴](#admin-users-force-leave-remote-room)
* [`admin users force-demote`↴](#admin-users-force-demote)
* [`admin users make-user-admin`↴](#admin-users-make-user-admin)
* [`admin users put-room-tag`↴](#admin-users-put-room-tag)
* [`admin users delete-room-tag`↴](#admin-users-delete-room-tag)
* [`admin users get-room-tags`↴](#admin-users-get-room-tags)
* [`admin users redact-event`↴](#admin-users-redact-event)
* [`admin users force-join-list-of-local-users`↴](#admin-users-force-join-list-of-local-users)
* [`admin users force-join-all-local-users`↴](#admin-users-force-join-all-local-users)
* [`admin token`↴](#admin-token)
* [`admin token issue`↴](#admin-token-issue)
* [`admin token revoke`↴](#admin-token-revoke)
* [`admin token list`↴](#admin-token-list)
* [`admin rooms`↴](#admin-rooms)
* [`admin rooms list-rooms`↴](#admin-rooms-list-rooms)
* [`admin rooms info`↴](#admin-rooms-info)
* [`admin rooms info list-joined-members`↴](#admin-rooms-info-list-joined-members)
* [`admin rooms info view-room-topic`↴](#admin-rooms-info-view-room-topic)
* [`admin rooms moderation`↴](#admin-rooms-moderation)
* [`admin rooms moderation ban-room`↴](#admin-rooms-moderation-ban-room)
* [`admin rooms moderation ban-list-of-rooms`↴](#admin-rooms-moderation-ban-list-of-rooms)
* [`admin rooms moderation unban-room`↴](#admin-rooms-moderation-unban-room)
* [`admin rooms moderation list-banned-rooms`↴](#admin-rooms-moderation-list-banned-rooms)
* [`admin rooms alias`↴](#admin-rooms-alias)
* [`admin rooms alias set`↴](#admin-rooms-alias-set)
* [`admin rooms alias remove`↴](#admin-rooms-alias-remove)
* [`admin rooms alias which`↴](#admin-rooms-alias-which)
* [`admin rooms alias list`↴](#admin-rooms-alias-list)
* [`admin rooms directory`↴](#admin-rooms-directory)
* [`admin rooms directory publish`↴](#admin-rooms-directory-publish)
* [`admin rooms directory unpublish`↴](#admin-rooms-directory-unpublish)
* [`admin rooms directory list`↴](#admin-rooms-directory-list)
* [`admin rooms exists`↴](#admin-rooms-exists)
* [`admin federation`↴](#admin-federation)
* [`admin federation incoming-federation`↴](#admin-federation-incoming-federation)
* [`admin federation disable-room`↴](#admin-federation-disable-room)
* [`admin federation enable-room`↴](#admin-federation-enable-room)
* [`admin federation fetch-support-well-known`↴](#admin-federation-fetch-support-well-known)
* [`admin federation remote-user-in-rooms`↴](#admin-federation-remote-user-in-rooms)
* [`admin server`↴](#admin-server)
* [`admin server uptime`↴](#admin-server-uptime)
* [`admin server show-config`↴](#admin-server-show-config)
* [`admin server reload-config`↴](#admin-server-reload-config)
* [`admin server list-features`↴](#admin-server-list-features)
* [`admin server memory-usage`↴](#admin-server-memory-usage)
* [`admin server clear-caches`↴](#admin-server-clear-caches)
* [`admin server backup-database`↴](#admin-server-backup-database)
* [`admin server list-backups`↴](#admin-server-list-backups)
* [`admin server admin-notice`↴](#admin-server-admin-notice)
* [`admin server reload-mods`↴](#admin-server-reload-mods)
* [`admin server restart`↴](#admin-server-restart)
* [`admin server shutdown`↴](#admin-server-shutdown)
* [`admin media`↴](#admin-media)
* [`admin media delete`↴](#admin-media-delete)
* [`admin media delete-list`↴](#admin-media-delete-list)
* [`admin media delete-past-remote-media`↴](#admin-media-delete-past-remote-media)
* [`admin media delete-all-from-user`↴](#admin-media-delete-all-from-user)
* [`admin media delete-all-from-server`↴](#admin-media-delete-all-from-server)
* [`admin media get-file-info`↴](#admin-media-get-file-info)
* [`admin media get-remote-file`↴](#admin-media-get-remote-file)
* [`admin media get-remote-thumbnail`↴](#admin-media-get-remote-thumbnail)
* [`admin check`↴](#admin-check)
* [`admin check check-all-users`↴](#admin-check-check-all-users)
* [`admin debug`↴](#admin-debug)
* [`admin debug echo`↴](#admin-debug-echo)
* [`admin debug get-auth-chain`↴](#admin-debug-get-auth-chain)
* [`admin debug parse-pdu`↴](#admin-debug-parse-pdu)
* [`admin debug get-pdu`↴](#admin-debug-get-pdu)
* [`admin debug get-short-pdu`↴](#admin-debug-get-short-pdu)
* [`admin debug get-remote-pdu`↴](#admin-debug-get-remote-pdu)
* [`admin debug get-remote-pdu-list`↴](#admin-debug-get-remote-pdu-list)
* [`admin debug get-room-state`↴](#admin-debug-get-room-state)
* [`admin debug get-signing-keys`↴](#admin-debug-get-signing-keys)
* [`admin debug get-verify-keys`↴](#admin-debug-get-verify-keys)
* [`admin debug ping`↴](#admin-debug-ping)
* [`admin debug force-device-list-updates`↴](#admin-debug-force-device-list-updates)
* [`admin debug change-log-level`↴](#admin-debug-change-log-level)
* [`admin debug verify-json`↴](#admin-debug-verify-json)
* [`admin debug verify-pdu`↴](#admin-debug-verify-pdu)
* [`admin debug first-pdu-in-room`↴](#admin-debug-first-pdu-in-room)
* [`admin debug latest-pdu-in-room`↴](#admin-debug-latest-pdu-in-room)
* [`admin debug force-set-room-state-from-server`↴](#admin-debug-force-set-room-state-from-server)
* [`admin debug resolve-true-destination`↴](#admin-debug-resolve-true-destination)
* [`admin debug memory-stats`↴](#admin-debug-memory-stats)
* [`admin debug runtime-metrics`↴](#admin-debug-runtime-metrics)
* [`admin debug runtime-interval`↴](#admin-debug-runtime-interval)
* [`admin debug time`↴](#admin-debug-time)
* [`admin debug list-dependencies`↴](#admin-debug-list-dependencies)
* [`admin debug database-stats`↴](#admin-debug-database-stats)
* [`admin debug trim-memory`↴](#admin-debug-trim-memory)
* [`admin debug database-files`↴](#admin-debug-database-files)
* [`admin query`↴](#admin-query)
* [`admin query account-data`↴](#admin-query-account-data)
* [`admin query account-data changes-since`↴](#admin-query-account-data-changes-since)
* [`admin query account-data account-data-get`↴](#admin-query-account-data-account-data-get)
* [`admin query appservice`↴](#admin-query-appservice)
* [`admin query appservice get-registration`↴](#admin-query-appservice-get-registration)
* [`admin query appservice all`↴](#admin-query-appservice-all)
* [`admin query presence`↴](#admin-query-presence)
* [`admin query presence get-presence`↴](#admin-query-presence-get-presence)
* [`admin query presence presence-since`↴](#admin-query-presence-presence-since)
* [`admin query room-alias`↴](#admin-query-room-alias)
* [`admin query room-alias resolve-local-alias`↴](#admin-query-room-alias-resolve-local-alias)
* [`admin query room-alias local-aliases-for-room`↴](#admin-query-room-alias-local-aliases-for-room)
* [`admin query room-alias all-local-aliases`↴](#admin-query-room-alias-all-local-aliases)
* [`admin query room-state-cache`↴](#admin-query-room-state-cache)
* [`admin query room-state-cache server-in-room`↴](#admin-query-room-state-cache-server-in-room)
* [`admin query room-state-cache room-servers`↴](#admin-query-room-state-cache-room-servers)
* [`admin query room-state-cache server-rooms`↴](#admin-query-room-state-cache-server-rooms)
* [`admin query room-state-cache room-members`↴](#admin-query-room-state-cache-room-members)
* [`admin query room-state-cache local-users-in-room`↴](#admin-query-room-state-cache-local-users-in-room)
* [`admin query room-state-cache active-local-users-in-room`↴](#admin-query-room-state-cache-active-local-users-in-room)
* [`admin query room-state-cache room-joined-count`↴](#admin-query-room-state-cache-room-joined-count)
* [`admin query room-state-cache room-invited-count`↴](#admin-query-room-state-cache-room-invited-count)
* [`admin query room-state-cache room-user-once-joined`↴](#admin-query-room-state-cache-room-user-once-joined)
* [`admin query room-state-cache room-members-invited`↴](#admin-query-room-state-cache-room-members-invited)
* [`admin query room-state-cache get-invite-count`↴](#admin-query-room-state-cache-get-invite-count)
* [`admin query room-state-cache get-left-count`↴](#admin-query-room-state-cache-get-left-count)
* [`admin query room-state-cache rooms-joined`↴](#admin-query-room-state-cache-rooms-joined)
* [`admin query room-state-cache rooms-left`↴](#admin-query-room-state-cache-rooms-left)
* [`admin query room-state-cache rooms-invited`↴](#admin-query-room-state-cache-rooms-invited)
* [`admin query room-state-cache invite-state`↴](#admin-query-room-state-cache-invite-state)
* [`admin query room-timeline`↴](#admin-query-room-timeline)
* [`admin query room-timeline pdus`↴](#admin-query-room-timeline-pdus)
* [`admin query room-timeline last`↴](#admin-query-room-timeline-last)
* [`admin query globals`↴](#admin-query-globals)
* [`admin query globals database-version`↴](#admin-query-globals-database-version)
* [`admin query globals current-count`↴](#admin-query-globals-current-count)
* [`admin query globals last-check-for-announcements-id`↴](#admin-query-globals-last-check-for-announcements-id)
* [`admin query globals signing-keys-for`↴](#admin-query-globals-signing-keys-for)
* [`admin query sending`↴](#admin-query-sending)
* [`admin query sending active-requests`↴](#admin-query-sending-active-requests)
* [`admin query sending active-requests-for`↴](#admin-query-sending-active-requests-for)
* [`admin query sending queued-requests`↴](#admin-query-sending-queued-requests)
* [`admin query sending get-latest-edu-count`↴](#admin-query-sending-get-latest-edu-count)
* [`admin query users`↴](#admin-query-users)
* [`admin query users count-users`↴](#admin-query-users-count-users)
* [`admin query users iter-users`↴](#admin-query-users-iter-users)
* [`admin query users iter-users2`↴](#admin-query-users-iter-users2)
* [`admin query users password-hash`↴](#admin-query-users-password-hash)
* [`admin query users list-devices`↴](#admin-query-users-list-devices)
* [`admin query users list-devices-metadata`↴](#admin-query-users-list-devices-metadata)
* [`admin query users get-device-metadata`↴](#admin-query-users-get-device-metadata)
* [`admin query users get-devices-version`↴](#admin-query-users-get-devices-version)
* [`admin query users count-one-time-keys`↴](#admin-query-users-count-one-time-keys)
* [`admin query users get-device-keys`↴](#admin-query-users-get-device-keys)
* [`admin query users get-user-signing-key`↴](#admin-query-users-get-user-signing-key)
* [`admin query users get-master-key`↴](#admin-query-users-get-master-key)
* [`admin query users get-to-device-events`↴](#admin-query-users-get-to-device-events)
* [`admin query users get-latest-backup`↴](#admin-query-users-get-latest-backup)
* [`admin query users get-latest-backup-version`↴](#admin-query-users-get-latest-backup-version)
* [`admin query users get-backup-algorithm`↴](#admin-query-users-get-backup-algorithm)
* [`admin query users get-all-backups`↴](#admin-query-users-get-all-backups)
* [`admin query users get-room-backups`↴](#admin-query-users-get-room-backups)
* [`admin query users get-backup-session`↴](#admin-query-users-get-backup-session)
* [`admin query users get-shared-rooms`↴](#admin-query-users-get-shared-rooms)
* [`admin query resolver`↴](#admin-query-resolver)
* [`admin query resolver destinations-cache`↴](#admin-query-resolver-destinations-cache)
* [`admin query resolver overrides-cache`↴](#admin-query-resolver-overrides-cache)
* [`admin query pusher`↴](#admin-query-pusher)
* [`admin query pusher get-pushers`↴](#admin-query-pusher-get-pushers)
* [`admin query short`↴](#admin-query-short)
* [`admin query short short-event-id`↴](#admin-query-short-short-event-id)
* [`admin query short short-room-id`↴](#admin-query-short-short-room-id)
* [`admin query raw`↴](#admin-query-raw)
* [`admin query raw raw-maps`↴](#admin-query-raw-raw-maps)
* [`admin query raw raw-get`↴](#admin-query-raw-raw-get)
* [`admin query raw raw-del`↴](#admin-query-raw-raw-del)
* [`admin query raw raw-keys`↴](#admin-query-raw-raw-keys)
* [`admin query raw raw-keys-sizes`↴](#admin-query-raw-raw-keys-sizes)
* [`admin query raw raw-keys-total`↴](#admin-query-raw-raw-keys-total)
* [`admin query raw raw-vals-sizes`↴](#admin-query-raw-raw-vals-sizes)
* [`admin query raw raw-vals-total`↴](#admin-query-raw-raw-vals-total)
* [`admin query raw raw-iter`↴](#admin-query-raw-raw-iter)
* [`admin query raw raw-keys-from`↴](#admin-query-raw-raw-keys-from)
* [`admin query raw raw-iter-from`↴](#admin-query-raw-raw-iter-from)
* [`admin query raw raw-count`↴](#admin-query-raw-raw-count)
* [`admin query raw compact`↴](#admin-query-raw-compact)

## `admin`

**Usage:** `admin <COMMAND>`

###### **Subcommands:**

* `appservices` — - Commands for managing appservices
* `users` — - Commands for managing local users
* `token` — - Commands for managing registration tokens
* `rooms` — - Commands for managing rooms
* `federation` — - Commands for managing federation
* `server` — - Commands for managing the server
* `media` — - Commands for managing media
* `check` — - Commands for checking integrity
* `debug` — - Commands for debugging things
* `query` — - Low-level queries for database getters and iterators



## `admin appservices`

- Commands for managing appservices

**Usage:** `admin appservices <COMMAND>`

###### **Subcommands:**

* `register` — - Register an appservice using its registration YAML
* `unregister` — - Unregister an appservice using its ID
* `show-appservice-config` — - Show an appservice's config using its ID
* `list-registered` — - List all the currently registered appservices



## `admin appservices register`

- Register an appservice using its registration YAML

This command needs a YAML generated by an appservice (such as a bridge), which must be provided in a Markdown code block below the command.

Registering a new bridge using the ID of an existing bridge will replace the old one.

**Usage:** `admin appservices register`



## `admin appservices unregister`

- Unregister an appservice using its ID

You can find the ID using the `list-appservices` command.

**Usage:** `admin appservices unregister <APPSERVICE_IDENTIFIER>`

###### **Arguments:**

* `<APPSERVICE_IDENTIFIER>` — The appservice to unregister



## `admin appservices show-appservice-config`

- Show an appservice's config using its ID

You can find the ID using the `list-appservices` command.

**Usage:** `admin appservices show-appservice-config <APPSERVICE_IDENTIFIER>`

###### **Arguments:**

* `<APPSERVICE_IDENTIFIER>` — The appservice to show



## `admin appservices list-registered`

- List all the currently registered appservices

**Usage:** `admin appservices list-registered`



## `admin users`

- Commands for managing local users

**Usage:** `admin users <COMMAND>`

###### **Subcommands:**

* `create-user` — - Create a new user
* `reset-password` — - Reset user password
* `deactivate` — - Deactivate a user
* `deactivate-all` — - Deactivate a list of users
* `logout` — - Forcefully log a user out of all of their devices
* `suspend` — - Suspend a user
* `unsuspend` — - Unsuspend a user
* `lock` — - Lock a user
* `unlock` — - Unlock a user
* `enable-login` — - Enable login for a user
* `disable-login` — - Disable login for a user
* `list-users` — - List local users in the database
* `list-joined-rooms` — - Lists all the rooms (local and remote) that the specified user is joined in
* `force-join-room` — - Manually join a local user to a room
* `force-leave-room` — - Manually leave a local user from a room
* `force-leave-remote-room` — - Manually leave a remote room for a local user
* `force-demote` — - Forces the specified user to drop their power levels to the room default, if their permissions allow and the auth check permits
* `make-user-admin` — - Grant server-admin privileges to a user
* `put-room-tag` — - Puts a room tag for the specified user and room ID
* `delete-room-tag` — - Deletes the room tag for the specified user and room ID
* `get-room-tags` — - Gets all the room tags for the specified user and room ID
* `redact-event` — - Attempts to forcefully redact the specified event ID from the sender user
* `force-join-list-of-local-users` — - Force joins a specified list of local users to join the specified room
* `force-join-all-local-users` — - Force joins all local users to the specified room



## `admin users create-user`

- Create a new user

**Usage:** `admin users create-user <USERNAME> [PASSWORD]`

###### **Arguments:**

* `<USERNAME>` — Username of the new user
* `<PASSWORD>` — Password of the new user, if unspecified one is generated



## `admin users reset-password`

- Reset user password

**Usage:** `admin users reset-password [OPTIONS] <USERNAME> [PASSWORD]`

###### **Arguments:**

* `<USERNAME>` — Username of the user for whom the password should be reset
* `<PASSWORD>` — New password for the user, if unspecified one is generated

###### **Options:**

* `-l`, `--logout` — Log out existing sessions



## `admin users deactivate`

- Deactivate a user

User will be removed from all rooms by default. Use --no-leave-rooms to not leave all rooms by default.

**Usage:** `admin users deactivate [OPTIONS] <USER_ID>`

###### **Arguments:**

* `<USER_ID>`

###### **Options:**

* `-n`, `--no-leave-rooms`



## `admin users deactivate-all`

- Deactivate a list of users

Recommended to use in conjunction with list-local-users.

Users will be removed from joined rooms by default.

Can be overridden with --no-leave-rooms.

Removing a mass amount of users from a room may cause a significant amount of leave events. The time to leave rooms may depend significantly on joined rooms and servers.

This command needs a newline separated list of users provided in a Markdown code block below the command.

**Usage:** `admin users deactivate-all [OPTIONS]`

###### **Options:**

* `-n`, `--no-leave-rooms` — Does not leave any rooms the user is in on deactivation
* `-f`, `--force` — Also deactivate admin accounts and will assume leave all rooms too



## `admin users logout`

- Forcefully log a user out of all of their devices.

This will invalidate all access tokens for the specified user, effectively logging them out from all sessions. Note that this is destructive and may result in data loss for the user, such as encryption keys. Use with caution. Can only be used in the admin room.

**Usage:** `admin users logout <USER_ID>`

###### **Arguments:**

* `<USER_ID>` — Username of the user to log out



## `admin users suspend`

- Suspend a user

Suspended users are able to log in, sync, and read messages, but are not able to send events nor redact them, cannot change their profile, and are unable to join, invite to, or knock on rooms.

Suspended users can still leave rooms and deactivate their account. Suspending them effectively makes them read-only.

**Usage:** `admin users suspend <USER_ID>`

###### **Arguments:**

* `<USER_ID>` — Username of the user to suspend



## `admin users unsuspend`

- Unsuspend a user

Reverses the effects of the `suspend` command, allowing the user to send messages, change their profile, create room invites, etc.

**Usage:** `admin users unsuspend <USER_ID>`

###### **Arguments:**

* `<USER_ID>` — Username of the user to unsuspend



## `admin users lock`

- Lock a user

Locked users are unable to use their accounts beyond logging out. This is akin to a temporary deactivation that does not change the user's password. This can be used to quickly prevent a user from accessing their account.

**Usage:** `admin users lock <USER_ID>`

###### **Arguments:**

* `<USER_ID>` — Username of the user to lock



## `admin users unlock`

- Unlock a user

Reverses the effects of the `lock` command, allowing the user to use their account again.

**Usage:** `admin users unlock <USER_ID>`

###### **Arguments:**

* `<USER_ID>` — Username of the user to unlock



## `admin users enable-login`

- Enable login for a user

**Usage:** `admin users enable-login <USER_ID>`

###### **Arguments:**

* `<USER_ID>` — Username of the user to enable login for



## `admin users disable-login`

- Disable login for a user

Disables login for the specified user without deactivating or locking their account. This prevents the user from obtaining new access tokens, but does not invalidate existing sessions.

**Usage:** `admin users disable-login <USER_ID>`

###### **Arguments:**

* `<USER_ID>` — Username of the user to disable login for



## `admin users list-users`

- List local users in the database

**Usage:** `admin users list-users`



## `admin users list-joined-rooms`

- Lists all the rooms (local and remote) that the specified user is joined in

**Usage:** `admin users list-joined-rooms <USER_ID>`

###### **Arguments:**

* `<USER_ID>`



## `admin users force-join-room`

- Manually join a local user to a room

**Usage:** `admin users force-join-room <USER_ID> <ROOM_ID>`

###### **Arguments:**

* `<USER_ID>`
* `<ROOM_ID>`



## `admin users force-leave-room`

- Manually leave a local user from a room

**Usage:** `admin users force-leave-room <USER_ID> <ROOM_ID>`

###### **Arguments:**

* `<USER_ID>`
* `<ROOM_ID>`



## `admin users force-leave-remote-room`

- Manually leave a remote room for a local user

**Usage:** `admin users force-leave-remote-room <USER_ID> <ROOM_ID> [VIA]`

###### **Arguments:**

* `<USER_ID>`
* `<ROOM_ID>`
* `<VIA>`



## `admin users force-demote`

- Forces the specified user to drop their power levels to the room default, if their permissions allow and the auth check permits

**Usage:** `admin users force-demote <USER_ID> <ROOM_ID>`

###### **Arguments:**

* `<USER_ID>`
* `<ROOM_ID>`



## `admin users make-user-admin`

- Grant server-admin privileges to a user

**Usage:** `admin users make-user-admin <USER_ID>`

###### **Arguments:**

* `<USER_ID>`



## `admin users put-room-tag`

- Puts a room tag for the specified user and room ID.

This is primarily useful if you'd like to set your admin room to the special "System Alerts" section in Element as a way to permanently see your admin room without it being buried away in your favourites or rooms. To do this, you would pass your user, your admin room's internal ID, and the tag name `m.server_notice`.

**Usage:** `admin users put-room-tag <USER_ID> <ROOM_ID> <TAG>`

###### **Arguments:**

* `<USER_ID>`
* `<ROOM_ID>`
* `<TAG>`



## `admin users delete-room-tag`

- Deletes the room tag for the specified user and room ID

**Usage:** `admin users delete-room-tag <USER_ID> <ROOM_ID> <TAG>`

###### **Arguments:**

* `<USER_ID>`
* `<ROOM_ID>`
* `<TAG>`



## `admin users get-room-tags`

- Gets all the room tags for the specified user and room ID

**Usage:** `admin users get-room-tags <USER_ID> <ROOM_ID>`

###### **Arguments:**

* `<USER_ID>`
* `<ROOM_ID>`



## `admin users redact-event`

- Attempts to forcefully redact the specified event ID from the sender user

This is only valid for local users

**Usage:** `admin users redact-event <EVENT_ID>`

###### **Arguments:**

* `<EVENT_ID>`



## `admin users force-join-list-of-local-users`

- Force joins a specified list of local users to join the specified room.

Specify a codeblock of usernames.

At least 1 server admin must be in the room to reduce abuse.

Requires the `--yes-i-want-to-do-this` flag.

**Usage:** `admin users force-join-list-of-local-users [OPTIONS] <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>`

###### **Options:**

* `--yes-i-want-to-do-this`



## `admin users force-join-all-local-users`

- Force joins all local users to the specified room.

At least 1 server admin must be in the room to reduce abuse.

Requires the `--yes-i-want-to-do-this` flag.

**Usage:** `admin users force-join-all-local-users [OPTIONS] <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>`

###### **Options:**

* `--yes-i-want-to-do-this`



## `admin token`

- Commands for managing registration tokens

**Usage:** `admin token <COMMAND>`

###### **Subcommands:**

* `issue` — - Issue a new registration token
* `revoke` — - Revoke a registration token
* `list` — - List all registration tokens



## `admin token issue`

- Issue a new registration token

**Usage:** `admin token issue <--max-uses <MAX_USES>|--max-age <MAX_AGE>|--immortal|--once>`

###### **Options:**

* `--max-uses <MAX_USES>` — The maximum number of times this token is allowed to be used before it expires
* `--max-age <MAX_AGE>` — The maximum age of this token (e.g. 30s, 5m, 7d). It will expire after this much time has passed
* `--immortal` — This token will never expire
* `--once` — A shortcut for `--max-uses 1`



## `admin token revoke`

- Revoke a registration token

**Usage:** `admin token revoke <TOKEN>`

###### **Arguments:**

* `<TOKEN>` — The token to revoke



## `admin token list`

- List all registration tokens

**Usage:** `admin token list`



## `admin rooms`

- Commands for managing rooms

**Usage:** `admin rooms <COMMAND>`

###### **Subcommands:**

* `list-rooms` — - List all rooms the server knows about
* `info` — - View information about a room we know about
* `moderation` — - Manage moderation of remote or local rooms
* `alias` — - Manage rooms' aliases
* `directory` — - Manage the room directory
* `exists` — - Check if we know about a room



## `admin rooms list-rooms`

- List all rooms the server knows about

**Usage:** `admin rooms list-rooms [OPTIONS] [PAGE]`

###### **Arguments:**

* `<PAGE>`

###### **Options:**

* `--exclude-disabled` — Excludes rooms that we have federation disabled with
* `--exclude-banned` — Excludes rooms that we have banned
* `--no-details` — Whether to only output room IDs without supplementary room information



## `admin rooms info`

- View information about a room we know about

**Usage:** `admin rooms info <COMMAND>`

###### **Subcommands:**

* `list-joined-members` — - List joined members in a room
* `view-room-topic` — - Displays room topic



## `admin rooms info list-joined-members`

- List joined members in a room

**Usage:** `admin rooms info list-joined-members [OPTIONS] <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>`

###### **Options:**

* `--local-only` — Lists only our local users in the specified room



## `admin rooms info view-room-topic`

- Displays room topic

Room topics can be huge, so this is in its own separate command

**Usage:** `admin rooms info view-room-topic <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>`



## `admin rooms moderation`

- Manage moderation of remote or local rooms

**Usage:** `admin rooms moderation <COMMAND>`

###### **Subcommands:**

* `ban-room` — - Bans a room from local users joining and evicts all our local users (including server admins) from the room. Also blocks any invites (local and remote) for the banned room, and disables federation entirely with it
* `ban-list-of-rooms` — - Bans a list of rooms (room IDs and room aliases) from a newline delimited codeblock similar to `user deactivate-all`. Applies the same steps as ban-room
* `unban-room` — - Unbans a room to allow local users to join again
* `list-banned-rooms` — - List of all rooms we have banned



## `admin rooms moderation ban-room`

- Bans a room from local users joining and evicts all our local users (including server admins) from the room. Also blocks any invites (local and remote) for the banned room, and disables federation entirely with it

**Usage:** `admin rooms moderation ban-room <ROOM>`

###### **Arguments:**

* `<ROOM>` — The room in the format of `!roomid:example.com` or a room alias in the format of `#roomalias:example.com`



## `admin rooms moderation ban-list-of-rooms`

- Bans a list of rooms (room IDs and room aliases) from a newline delimited codeblock similar to `user deactivate-all`. Applies the same steps as ban-room

**Usage:** `admin rooms moderation ban-list-of-rooms`



## `admin rooms moderation unban-room`

- Unbans a room to allow local users to join again

**Usage:** `admin rooms moderation unban-room <ROOM>`

###### **Arguments:**

* `<ROOM>` — The room in the format of `!roomid:example.com` or a room alias in the format of `#roomalias:example.com`



## `admin rooms moderation list-banned-rooms`

- List of all rooms we have banned

**Usage:** `admin rooms moderation list-banned-rooms [OPTIONS]`

###### **Options:**

* `--no-details` — Whether to only output room IDs without supplementary room information



## `admin rooms alias`

- Manage rooms' aliases

**Usage:** `admin rooms alias <COMMAND>`

###### **Subcommands:**

* `set` — - Make an alias point to a room
* `remove` — - Remove a local alias
* `which` — - Show which room is using an alias
* `list` — - List aliases currently being used



## `admin rooms alias set`

- Make an alias point to a room

**Usage:** `admin rooms alias set [OPTIONS] <ROOM_ID> <ROOM_ALIAS_LOCALPART>`

###### **Arguments:**

* `<ROOM_ID>` — The room id to set the alias on
* `<ROOM_ALIAS_LOCALPART>` — The alias localpart to use (`alias`, not `#alias:servername.tld`)

###### **Options:**

* `-f`, `--force` — Set the alias even if a room is already using it



## `admin rooms alias remove`

- Remove a local alias

**Usage:** `admin rooms alias remove <ROOM_ALIAS_LOCALPART>`

###### **Arguments:**

* `<ROOM_ALIAS_LOCALPART>` — The alias localpart to remove (`alias`, not `#alias:servername.tld`)



## `admin rooms alias which`

- Show which room is using an alias

**Usage:** `admin rooms alias which <ROOM_ALIAS_LOCALPART>`

###### **Arguments:**

* `<ROOM_ALIAS_LOCALPART>` — The alias localpart to look up (`alias`, not `#alias:servername.tld`)



## `admin rooms alias list`

- List aliases currently being used

**Usage:** `admin rooms alias list [ROOM_ID]`

###### **Arguments:**

* `<ROOM_ID>` — If set, only list the aliases for this room



## `admin rooms directory`

- Manage the room directory

**Usage:** `admin rooms directory <COMMAND>`

###### **Subcommands:**

* `publish` — - Publish a room to the room directory
* `unpublish` — - Unpublish a room to the room directory
* `list` — - List rooms that are published



## `admin rooms directory publish`

- Publish a room to the room directory

**Usage:** `admin rooms directory publish <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>` — The room id of the room to publish



## `admin rooms directory unpublish`

- Unpublish a room to the room directory

**Usage:** `admin rooms directory unpublish <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>` — The room id of the room to unpublish



## `admin rooms directory list`

- List rooms that are published

**Usage:** `admin rooms directory list [PAGE]`

###### **Arguments:**

* `<PAGE>`



## `admin rooms exists`

- Check if we know about a room

**Usage:** `admin rooms exists <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>`



## `admin federation`

- Commands for managing federation

**Usage:** `admin federation <COMMAND>`

###### **Subcommands:**

* `incoming-federation` — - List all rooms we are currently handling an incoming pdu from
* `disable-room` — - Disables incoming federation handling for a room
* `enable-room` — - Enables incoming federation handling for a room again
* `fetch-support-well-known` — - Fetch `/.well-known/matrix/support` from the specified server
* `remote-user-in-rooms` — - Lists all the rooms we share/track with the specified *remote* user



## `admin federation incoming-federation`

- List all rooms we are currently handling an incoming pdu from

**Usage:** `admin federation incoming-federation`



## `admin federation disable-room`

- Disables incoming federation handling for a room

**Usage:** `admin federation disable-room <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>`



## `admin federation enable-room`

- Enables incoming federation handling for a room again

**Usage:** `admin federation enable-room <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>`



## `admin federation fetch-support-well-known`

- Fetch `/.well-known/matrix/support` from the specified server

Despite the name, this is not a federation endpoint and does not go through the federation / server resolution process as per-spec this is supposed to be served at the server_name.

Respecting homeservers put this file here for listing administration, moderation, and security inquiries. This command provides a way to easily fetch that information.

**Usage:** `admin federation fetch-support-well-known <SERVER_NAME>`

###### **Arguments:**

* `<SERVER_NAME>`



## `admin federation remote-user-in-rooms`

- Lists all the rooms we share/track with the specified *remote* user

**Usage:** `admin federation remote-user-in-rooms <USER_ID>`

###### **Arguments:**

* `<USER_ID>`



## `admin server`

- Commands for managing the server

**Usage:** `admin server <COMMAND>`

###### **Subcommands:**

* `uptime` — - Time elapsed since startup
* `show-config` — - Show configuration values
* `reload-config` — - Reload configuration values
* `list-features` — - List the features built into the server
* `memory-usage` — - Print database memory usage statistics
* `clear-caches` — - Clears all of Continuwuity's caches
* `backup-database` — - Performs an online backup of the database (only available for RocksDB at the moment)
* `list-backups` — - List database backups
* `admin-notice` — - Send a message to the admin room
* `reload-mods` — - Hot-reload the server
* `restart` — - Restart the server
* `shutdown` — - Shutdown the server



## `admin server uptime`

- Time elapsed since startup

**Usage:** `admin server uptime`



## `admin server show-config`

- Show configuration values

**Usage:** `admin server show-config`



## `admin server reload-config`

- Reload configuration values

**Usage:** `admin server reload-config [PATH]`

###### **Arguments:**

* `<PATH>`



## `admin server list-features`

- List the features built into the server

**Usage:** `admin server list-features [OPTIONS]`

###### **Options:**

* `-a`, `--available`
* `-e`, `--enabled`
* `-c`, `--comma`



## `admin server memory-usage`

- Print database memory usage statistics

**Usage:** `admin server memory-usage`



## `admin server clear-caches`

- Clears all of Continuwuity's caches

**Usage:** `admin server clear-caches`



## `admin server backup-database`

- Performs an online backup of the database (only available for RocksDB at the moment)

**Usage:** `admin server backup-database`



## `admin server list-backups`

- List database backups

**Usage:** `admin server list-backups`



## `admin server admin-notice`

- Send a message to the admin room

**Usage:** `admin server admin-notice [MESSAGE]...`

###### **Arguments:**

* `<MESSAGE>`



## `admin server reload-mods`

- Hot-reload the server

**Usage:** `admin server reload-mods`



## `admin server restart`

- Restart the server

**Usage:** `admin server restart [OPTIONS]`

###### **Options:**

* `-f`, `--force`



## `admin server shutdown`

- Shutdown the server

**Usage:** `admin server shutdown`



## `admin media`

- Commands for managing media

**Usage:** `admin media <COMMAND>`

###### **Subcommands:**

* `delete` — - Deletes a single media file from our database and on the filesystem via a single MXC URL or event ID (not redacted)
* `delete-list` — - Deletes a codeblock list of MXC URLs from our database and on the filesystem. This will always ignore errors
* `delete-past-remote-media` — Deletes all remote (and optionally local) media created before/after
[duration] ago, using filesystem metadata first created at date, or
fallback to last modified date. This will always ignore errors by
default.
* `delete-all-from-user` — - Deletes all the local media from a local user on our server. This will always ignore errors by default
* `delete-all-from-server` — - Deletes all remote media from the specified remote server. This will always ignore errors by default
* `get-file-info` —
* `get-remote-file` —
* `get-remote-thumbnail` —



## `admin media delete`

- Deletes a single media file from our database and on the filesystem via a single MXC URL or event ID (not redacted)

**Usage:** `admin media delete [OPTIONS]`

###### **Options:**

* `--mxc <MXC>` — The MXC URL to delete
* `--event-id <EVENT_ID>` — - The message event ID which contains the media and thumbnail MXC URLs



## `admin media delete-list`

- Deletes a codeblock list of MXC URLs from our database and on the filesystem. This will always ignore errors

**Usage:** `admin media delete-list`



## `admin media delete-past-remote-media`

Deletes all remote (and optionally local) media created before/after
[duration] ago, using filesystem metadata first created at date, or
fallback to last modified date. This will always ignore errors by
default.

* Examples:
  * Delete all remote media older than a year:

    `!admin media delete-past-remote-media -b 1y`

  * Delete all remote and local media from 3 days ago, up until now:

    `!admin media delete-past-remote-media -a 3d
--yes-i-want-to-delete-local-media`

**Usage:** `admin media delete-past-remote-media [OPTIONS] <DURATION>`

###### **Arguments:**

* `<DURATION>` — - The relative time (e.g. 30s, 5m, 7d) from now within which to search

###### **Options:**

* `-b`, `--before` — - Only delete media created before [duration] ago
* `-a`, `--after` — - Only delete media created after [duration] ago
* `--yes-i-want-to-delete-local-media` — - Long argument to additionally delete local media



## `admin media delete-all-from-user`

- Deletes all the local media from a local user on our server. This will always ignore errors by default

**Usage:** `admin media delete-all-from-user <USERNAME>`

###### **Arguments:**

* `<USERNAME>`



## `admin media delete-all-from-server`

- Deletes all remote media from the specified remote server. This will always ignore errors by default

**Usage:** `admin media delete-all-from-server [OPTIONS] <SERVER_NAME>`

###### **Arguments:**

* `<SERVER_NAME>`

###### **Options:**

* `--yes-i-want-to-delete-local-media` — Long argument to delete local media



## `admin media get-file-info`

**Usage:** `admin media get-file-info <MXC>`

###### **Arguments:**

* `<MXC>` — The MXC URL to lookup info for



## `admin media get-remote-file`

**Usage:** `admin media get-remote-file [OPTIONS] <MXC>`

###### **Arguments:**

* `<MXC>` — The MXC URL to fetch

###### **Options:**

* `-s`, `--server <SERVER>`
* `-t`, `--timeout <TIMEOUT>`

  Default value: `10000`



## `admin media get-remote-thumbnail`

**Usage:** `admin media get-remote-thumbnail [OPTIONS] <MXC>`

###### **Arguments:**

* `<MXC>` — The MXC URL to fetch

###### **Options:**

* `-s`, `--server <SERVER>`
* `-t`, `--timeout <TIMEOUT>`

  Default value: `10000`
* `--width <WIDTH>`

  Default value: `800`
* `--height <HEIGHT>`

  Default value: `800`



## `admin check`

- Commands for checking integrity

**Usage:** `admin check <COMMAND>`

###### **Subcommands:**

* `check-all-users` —



## `admin check check-all-users`

**Usage:** `admin check check-all-users`



## `admin debug`

- Commands for debugging things

**Usage:** `admin debug <COMMAND>`

###### **Subcommands:**

* `echo` — - Echo input of admin command
* `get-auth-chain` — - Get the auth_chain of a PDU
* `parse-pdu` — - Parse and print a PDU from a JSON
* `get-pdu` — - Retrieve and print a PDU by EventID from the Continuwuity database
* `get-short-pdu` — - Retrieve and print a PDU by PduId from the Continuwuity database
* `get-remote-pdu` — - Attempts to retrieve a PDU from a remote server. **Does not** insert it into the database or persist it anywhere
* `get-remote-pdu-list` — - Same as `get-remote-pdu` but accepts a codeblock newline delimited list of PDUs and a single server to fetch from
* `get-room-state` — - Gets all the room state events for the specified room
* `get-signing-keys` — - Get and display signing keys from local cache or remote server
* `get-verify-keys` — - Get and display signing keys from local cache or remote server
* `ping` — - Sends a federation request to the remote server's `/_matrix/federation/v1/version` endpoint and measures the latency it took for the server to respond
* `force-device-list-updates` — - Forces device lists for all local and remote users to be updated (as having new keys available)
* `change-log-level` — - Change tracing log level/filter on the fly
* `verify-json` — - Verify JSON signatures
* `verify-pdu` — - Verify PDU
* `first-pdu-in-room` — - Prints the very first PDU in the specified room (typically m.room.create)
* `latest-pdu-in-room` — - Prints the latest ("last") PDU in the specified room (typically a message)
* `force-set-room-state-from-server` — - Forcefully replaces the room state of our local copy of the specified room, with the copy (auth chain and room state events) the specified remote server says
* `resolve-true-destination` — - Runs a server name through Continuwuity's true destination resolution process
* `memory-stats` — - Print extended memory usage
* `runtime-metrics` — - Print general tokio runtime metric totals
* `runtime-interval` — - Print detailed tokio runtime metrics accumulated since last command invocation
* `time` — - Print the current time
* `list-dependencies` — - List dependencies
* `database-stats` — - Get database statistics
* `trim-memory` — - Trim memory usage
* `database-files` — - List database files



## `admin debug echo`

- Echo input of admin command

**Usage:** `admin debug echo [MESSAGE]...`

###### **Arguments:**

* `<MESSAGE>`



## `admin debug get-auth-chain`

- Get the auth_chain of a PDU

**Usage:** `admin debug get-auth-chain <EVENT_ID>`

###### **Arguments:**

* `<EVENT_ID>` — An event ID (the $ character followed by the base64 reference hash)



## `admin debug parse-pdu`

- Parse and print a PDU from a JSON

The PDU event is only checked for validity and is not added to the database.

This command needs a JSON blob provided in a Markdown code block below the command.

**Usage:** `admin debug parse-pdu`



## `admin debug get-pdu`

- Retrieve and print a PDU by EventID from the Continuwuity database

**Usage:** `admin debug get-pdu <EVENT_ID>`

###### **Arguments:**

* `<EVENT_ID>` — An event ID (a $ followed by the base64 reference hash)



## `admin debug get-short-pdu`

- Retrieve and print a PDU by PduId from the Continuwuity database

**Usage:** `admin debug get-short-pdu <SHORTROOMID> <SHORTEVENTID>`

###### **Arguments:**

* `<SHORTROOMID>` — Shortroomid integer
* `<SHORTEVENTID>` — Shorteventid integer



## `admin debug get-remote-pdu`

- Attempts to retrieve a PDU from a remote server. **Does not** insert it into the database or persist it anywhere

**Usage:** `admin debug get-remote-pdu <EVENT_ID> <SERVER>`

###### **Arguments:**

* `<EVENT_ID>` — An event ID (a $ followed by the base64 reference hash)
* `<SERVER>` — Argument for us to attempt to fetch the event from the specified remote server



## `admin debug get-remote-pdu-list`

- Same as `get-remote-pdu` but accepts a codeblock newline delimited list of PDUs and a single server to fetch from

**Usage:** `admin debug get-remote-pdu-list [OPTIONS] <SERVER>`

###### **Arguments:**

* `<SERVER>` — Argument for us to attempt to fetch all the events from the specified remote server

###### **Options:**

* `-f`, `--force` — If set, ignores errors, else stops at the first error/failure



## `admin debug get-room-state`

- Gets all the room state events for the specified room.

This is functionally equivalent to `GET /_matrix/client/v3/rooms/{roomid}/state`, except the admin command does *not* check if the sender user is allowed to see state events. This is done because it's implied that server admins here have database access and can see/get room info themselves anyways if they were malicious admins.

Of course the check is still done on the actual client API.

**Usage:** `admin debug get-room-state <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>` — Room ID



## `admin debug get-signing-keys`

- Get and display signing keys from local cache or remote server

**Usage:** `admin debug get-signing-keys [OPTIONS] [SERVER_NAME]`

###### **Arguments:**

* `<SERVER_NAME>`

###### **Options:**

* `--notary <NOTARY>`
* `-q`, `--query`



## `admin debug get-verify-keys`

- Get and display signing keys from local cache or remote server

**Usage:** `admin debug get-verify-keys [SERVER_NAME]`

###### **Arguments:**

* `<SERVER_NAME>`



## `admin debug ping`

- Sends a federation request to the remote server's `/_matrix/federation/v1/version` endpoint and measures the latency it took for the server to respond

**Usage:** `admin debug ping <SERVER>`

###### **Arguments:**

* `<SERVER>`



## `admin debug force-device-list-updates`

- Forces device lists for all local and remote users to be updated (as having new keys available)

**Usage:** `admin debug force-device-list-updates`



## `admin debug change-log-level`

- Change tracing log level/filter on the fly

This accepts the same format as the `log` config option.

**Usage:** `admin debug change-log-level [OPTIONS] [FILTER]`

###### **Arguments:**

* `<FILTER>` — Log level/filter

###### **Options:**

* `-r`, `--reset` — Resets the log level/filter to the one in your config



## `admin debug verify-json`

- Verify JSON signatures

This command needs a JSON blob provided in a Markdown code block below the command.

**Usage:** `admin debug verify-json`



## `admin debug verify-pdu`

- Verify PDU

This re-verifies a PDU existing in the database found by ID.

**Usage:** `admin debug verify-pdu <EVENT_ID>`

###### **Arguments:**

* `<EVENT_ID>`



## `admin debug first-pdu-in-room`

- Prints the very first PDU in the specified room (typically m.room.create)

**Usage:** `admin debug first-pdu-in-room <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>` — The room ID



## `admin debug latest-pdu-in-room`

- Prints the latest ("last") PDU in the specified room (typically a message)

**Usage:** `admin debug latest-pdu-in-room <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>` — The room ID



## `admin debug force-set-room-state-from-server`

- Forcefully replaces the room state of our local copy of the specified room, with the copy (auth chain and room state events) the specified remote server says.

A common desire for room deletion is to simply "reset" our copy of the room. While this admin command is not a replacement for that, if you know you have split/broken room state and you know another server in the room that has the best/working room state, this command can let you use their room state. Such example is your server saying users are in a room, but other servers are saying they're not in the room in question.

This command will get the latest PDU in the room we know about, and request the room state at that point in time via `/_matrix/federation/v1/state/{roomId}`.

**Usage:** `admin debug force-set-room-state-from-server <ROOM_ID> <SERVER_NAME> [EVENT_ID]`

###### **Arguments:**

* `<ROOM_ID>` — The impacted room ID
* `<SERVER_NAME>` — The server we will use to query the room state for
* `<EVENT_ID>` — The event ID of the latest known PDU in the room. Will be found automatically if not provided



## `admin debug resolve-true-destination`

- Runs a server name through Continuwuity's true destination resolution process

Useful for debugging well-known issues

**Usage:** `admin debug resolve-true-destination [OPTIONS] <SERVER_NAME>`

###### **Arguments:**

* `<SERVER_NAME>`

###### **Options:**

* `-n`, `--no-cache`



## `admin debug memory-stats`

- Print extended memory usage

Optional argument is a character mask (a sequence of characters in any order) which enable additional extended statistics. Known characters are "abdeglmx". For convenience, a '*' will enable everything.

**Usage:** `admin debug memory-stats [OPTS]`

###### **Arguments:**

* `<OPTS>`



## `admin debug runtime-metrics`

- Print general tokio runtime metric totals

**Usage:** `admin debug runtime-metrics`



## `admin debug runtime-interval`

- Print detailed tokio runtime metrics accumulated since last command invocation

**Usage:** `admin debug runtime-interval`



## `admin debug time`

- Print the current time

**Usage:** `admin debug time`



## `admin debug list-dependencies`

- List dependencies

**Usage:** `admin debug list-dependencies [OPTIONS]`

###### **Options:**

* `-n`, `--names`



## `admin debug database-stats`

- Get database statistics

**Usage:** `admin debug database-stats [OPTIONS] [PROPERTY]`

###### **Arguments:**

* `<PROPERTY>`

###### **Options:**

* `-m`, `--map <MAP>`



## `admin debug trim-memory`

- Trim memory usage

**Usage:** `admin debug trim-memory`



## `admin debug database-files`

- List database files

**Usage:** `admin debug database-files [OPTIONS] [MAP]`

###### **Arguments:**

* `<MAP>`

###### **Options:**

* `--level <LEVEL>`



## `admin query`

- Low-level queries for database getters and iterators

**Usage:** `admin query <COMMAND>`

###### **Subcommands:**

* `account-data` — - account_data.rs iterators and getters
* `appservice` — - appservice.rs iterators and getters
* `presence` — - presence.rs iterators and getters
* `room-alias` — - rooms/alias.rs iterators and getters
* `room-state-cache` — - rooms/state_cache iterators and getters
* `room-timeline` — - rooms/timeline iterators and getters
* `globals` — - globals.rs iterators and getters
* `sending` — - sending.rs iterators and getters
* `users` — - users.rs iterators and getters
* `resolver` — - resolver service
* `pusher` — - pusher service
* `short` — - short service
* `raw` — - raw service



## `admin query account-data`

- account_data.rs iterators and getters

**Usage:** `admin query account-data <COMMAND>`

###### **Subcommands:**

* `changes-since` — - Returns all changes to the account data that happened after `since`
* `account-data-get` — - Searches the account data for a specific kind



## `admin query account-data changes-since`

- Returns all changes to the account data that happened after `since`

**Usage:** `admin query account-data changes-since <USER_ID> <SINCE> [ROOM_ID]`

###### **Arguments:**

* `<USER_ID>` — Full user ID
* `<SINCE>` — UNIX timestamp since (u64)
* `<ROOM_ID>` — Optional room ID of the account data



## `admin query account-data account-data-get`

- Searches the account data for a specific kind

**Usage:** `admin query account-data account-data-get <USER_ID> <KIND> [ROOM_ID]`

###### **Arguments:**

* `<USER_ID>` — Full user ID
* `<KIND>` — Account data event type
* `<ROOM_ID>` — Optional room ID of the account data



## `admin query appservice`

- appservice.rs iterators and getters

**Usage:** `admin query appservice <COMMAND>`

###### **Subcommands:**

* `get-registration` — - Gets the appservice registration info/details from the ID as a string
* `all` — - Gets all appservice registrations with their ID and registration info



## `admin query appservice get-registration`

- Gets the appservice registration info/details from the ID as a string

**Usage:** `admin query appservice get-registration <APPSERVICE_ID>`

###### **Arguments:**

* `<APPSERVICE_ID>` — Appservice registration ID



## `admin query appservice all`

- Gets all appservice registrations with their ID and registration info

**Usage:** `admin query appservice all`



## `admin query presence`

- presence.rs iterators and getters

**Usage:** `admin query presence <COMMAND>`

###### **Subcommands:**

* `get-presence` — - Returns the latest presence event for the given user
* `presence-since` — - Iterator of the most recent presence updates that happened after the event with id `since`



## `admin query presence get-presence`

- Returns the latest presence event for the given user

**Usage:** `admin query presence get-presence <USER_ID>`

###### **Arguments:**

* `<USER_ID>` — Full user ID



## `admin query presence presence-since`

- Iterator of the most recent presence updates that happened after the event with id `since`

**Usage:** `admin query presence presence-since <SINCE>`

###### **Arguments:**

* `<SINCE>` — UNIX timestamp since (u64)



## `admin query room-alias`

- rooms/alias.rs iterators and getters

**Usage:** `admin query room-alias <COMMAND>`

###### **Subcommands:**

* `resolve-local-alias` —
* `local-aliases-for-room` — - Iterator of all our local room aliases for the room ID
* `all-local-aliases` — - Iterator of all our local aliases in our database with their room IDs



## `admin query room-alias resolve-local-alias`

**Usage:** `admin query room-alias resolve-local-alias <ALIAS>`

###### **Arguments:**

* `<ALIAS>` — Full room alias



## `admin query room-alias local-aliases-for-room`

- Iterator of all our local room aliases for the room ID

**Usage:** `admin query room-alias local-aliases-for-room <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>` — Full room ID



## `admin query room-alias all-local-aliases`

- Iterator of all our local aliases in our database with their room IDs

**Usage:** `admin query room-alias all-local-aliases`



## `admin query room-state-cache`

- rooms/state_cache iterators and getters

**Usage:** `admin query room-state-cache <COMMAND>`

###### **Subcommands:**

* `server-in-room` —
* `room-servers` —
* `server-rooms` —
* `room-members` —
* `local-users-in-room` —
* `active-local-users-in-room` —
* `room-joined-count` —
* `room-invited-count` —
* `room-user-once-joined` —
* `room-members-invited` —
* `get-invite-count` —
* `get-left-count` —
* `rooms-joined` —
* `rooms-left` —
* `rooms-invited` —
* `invite-state` —



## `admin query room-state-cache server-in-room`

**Usage:** `admin query room-state-cache server-in-room <SERVER> <ROOM_ID>`

###### **Arguments:**

* `<SERVER>`
* `<ROOM_ID>`



## `admin query room-state-cache room-servers`

**Usage:** `admin query room-state-cache room-servers <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>`



## `admin query room-state-cache server-rooms`

**Usage:** `admin query room-state-cache server-rooms <SERVER>`

###### **Arguments:**

* `<SERVER>`



## `admin query room-state-cache room-members`

**Usage:** `admin query room-state-cache room-members <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>`



## `admin query room-state-cache local-users-in-room`

**Usage:** `admin query room-state-cache local-users-in-room <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>`



## `admin query room-state-cache active-local-users-in-room`

**Usage:** `admin query room-state-cache active-local-users-in-room <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>`



## `admin query room-state-cache room-joined-count`

**Usage:** `admin query room-state-cache room-joined-count <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>`



## `admin query room-state-cache room-invited-count`

**Usage:** `admin query room-state-cache room-invited-count <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>`



## `admin query room-state-cache room-user-once-joined`

**Usage:** `admin query room-state-cache room-user-once-joined <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>`



## `admin query room-state-cache room-members-invited`

**Usage:** `admin query room-state-cache room-members-invited <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>`



## `admin query room-state-cache get-invite-count`

**Usage:** `admin query room-state-cache get-invite-count <ROOM_ID> <USER_ID>`

###### **Arguments:**

* `<ROOM_ID>`
* `<USER_ID>`



## `admin query room-state-cache get-left-count`

**Usage:** `admin query room-state-cache get-left-count <ROOM_ID> <USER_ID>`

###### **Arguments:**

* `<ROOM_ID>`
* `<USER_ID>`



## `admin query room-state-cache rooms-joined`

**Usage:** `admin query room-state-cache rooms-joined <USER_ID>`

###### **Arguments:**

* `<USER_ID>`



## `admin query room-state-cache rooms-left`

**Usage:** `admin query room-state-cache rooms-left <USER_ID>`

###### **Arguments:**

* `<USER_ID>`



## `admin query room-state-cache rooms-invited`

**Usage:** `admin query room-state-cache rooms-invited <USER_ID>`

###### **Arguments:**

* `<USER_ID>`



## `admin query room-state-cache invite-state`

**Usage:** `admin query room-state-cache invite-state <USER_ID> <ROOM_ID>`

###### **Arguments:**

* `<USER_ID>`
* `<ROOM_ID>`



## `admin query room-timeline`

- rooms/timeline iterators and getters

**Usage:** `admin query room-timeline <COMMAND>`

###### **Subcommands:**

* `pdus` —
* `last` —



## `admin query room-timeline pdus`

**Usage:** `admin query room-timeline pdus [OPTIONS] <ROOM_ID> [FROM]`

###### **Arguments:**

* `<ROOM_ID>`
* `<FROM>`

###### **Options:**

* `-l`, `--limit <LIMIT>`



## `admin query room-timeline last`

**Usage:** `admin query room-timeline last <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>`



## `admin query globals`

- globals.rs iterators and getters

**Usage:** `admin query globals <COMMAND>`

###### **Subcommands:**

* `database-version` —
* `current-count` —
* `last-check-for-announcements-id` —
* `signing-keys-for` — - This returns an empty `Ok(BTreeMap<..>)` when there are no keys found for the server



## `admin query globals database-version`

**Usage:** `admin query globals database-version`



## `admin query globals current-count`

**Usage:** `admin query globals current-count`



## `admin query globals last-check-for-announcements-id`

**Usage:** `admin query globals last-check-for-announcements-id`



## `admin query globals signing-keys-for`

- This returns an empty `Ok(BTreeMap<..>)` when there are no keys found for the server

**Usage:** `admin query globals signing-keys-for <ORIGIN>`

###### **Arguments:**

* `<ORIGIN>`



## `admin query sending`

- sending.rs iterators and getters

**Usage:** `admin query sending <COMMAND>`

###### **Subcommands:**

* `active-requests` — - Queries database for all `servercurrentevent_data`
* `active-requests-for` — - Queries database for `servercurrentevent_data` but for a specific destination
* `queued-requests` — - Queries database for `servernameevent_data` which are the queued up requests that will eventually be sent
* `get-latest-edu-count` —



## `admin query sending active-requests`

- Queries database for all `servercurrentevent_data`

**Usage:** `admin query sending active-requests`



## `admin query sending active-requests-for`

- Queries database for `servercurrentevent_data` but for a specific destination

This command takes only *one* format of these arguments:

appservice_id server_name user_id AND push_key

See src/service/sending/mod.rs for the definition of the `Destination` enum

**Usage:** `admin query sending active-requests-for [OPTIONS]`

###### **Options:**

* `-a`, `--appservice-id <APPSERVICE_ID>`
* `-s`, `--server-name <SERVER_NAME>`
* `-u`, `--user-id <USER_ID>`
* `-p`, `--push-key <PUSH_KEY>`



## `admin query sending queued-requests`

- Queries database for `servernameevent_data` which are the queued up requests that will eventually be sent

This command takes only *one* format of these arguments:

appservice_id server_name user_id AND push_key

See src/service/sending/mod.rs for the definition of the `Destination` enum

**Usage:** `admin query sending queued-requests [OPTIONS]`

###### **Options:**

* `-a`, `--appservice-id <APPSERVICE_ID>`
* `-s`, `--server-name <SERVER_NAME>`
* `-u`, `--user-id <USER_ID>`
* `-p`, `--push-key <PUSH_KEY>`



## `admin query sending get-latest-edu-count`

**Usage:** `admin query sending get-latest-edu-count <SERVER_NAME>`

###### **Arguments:**

* `<SERVER_NAME>`



## `admin query users`

- users.rs iterators and getters

**Usage:** `admin query users <COMMAND>`

###### **Subcommands:**

* `count-users` —
* `iter-users` —
* `iter-users2` —
* `password-hash` —
* `list-devices` —
* `list-devices-metadata` —
* `get-device-metadata` —
* `get-devices-version` —
* `count-one-time-keys` —
* `get-device-keys` —
* `get-user-signing-key` —
* `get-master-key` —
* `get-to-device-events` —
* `get-latest-backup` —
* `get-latest-backup-version` —
* `get-backup-algorithm` —
* `get-all-backups` —
* `get-room-backups` —
* `get-backup-session` —
* `get-shared-rooms` —



## `admin query users count-users`

**Usage:** `admin query users count-users`



## `admin query users iter-users`

**Usage:** `admin query users iter-users`



## `admin query users iter-users2`

**Usage:** `admin query users iter-users2`



## `admin query users password-hash`

**Usage:** `admin query users password-hash <USER_ID>`

###### **Arguments:**

* `<USER_ID>`



## `admin query users list-devices`

**Usage:** `admin query users list-devices <USER_ID>`

###### **Arguments:**

* `<USER_ID>`



## `admin query users list-devices-metadata`

**Usage:** `admin query users list-devices-metadata <USER_ID>`

###### **Arguments:**

* `<USER_ID>`



## `admin query users get-device-metadata`

**Usage:** `admin query users get-device-metadata <USER_ID> <DEVICE_ID>`

###### **Arguments:**

* `<USER_ID>`
* `<DEVICE_ID>`



## `admin query users get-devices-version`

**Usage:** `admin query users get-devices-version <USER_ID>`

###### **Arguments:**

* `<USER_ID>`



## `admin query users count-one-time-keys`

**Usage:** `admin query users count-one-time-keys <USER_ID> <DEVICE_ID>`

###### **Arguments:**

* `<USER_ID>`
* `<DEVICE_ID>`



## `admin query users get-device-keys`

**Usage:** `admin query users get-device-keys <USER_ID> <DEVICE_ID>`

###### **Arguments:**

* `<USER_ID>`
* `<DEVICE_ID>`



## `admin query users get-user-signing-key`

**Usage:** `admin query users get-user-signing-key <USER_ID>`

###### **Arguments:**

* `<USER_ID>`



## `admin query users get-master-key`

**Usage:** `admin query users get-master-key <USER_ID>`

###### **Arguments:**

* `<USER_ID>`



## `admin query users get-to-device-events`

**Usage:** `admin query users get-to-device-events <USER_ID> <DEVICE_ID>`

###### **Arguments:**

* `<USER_ID>`
* `<DEVICE_ID>`



## `admin query users get-latest-backup`

**Usage:** `admin query users get-latest-backup <USER_ID>`

###### **Arguments:**

* `<USER_ID>`



## `admin query users get-latest-backup-version`

**Usage:** `admin query users get-latest-backup-version <USER_ID>`

###### **Arguments:**

* `<USER_ID>`



## `admin query users get-backup-algorithm`

**Usage:** `admin query users get-backup-algorithm <USER_ID> <VERSION>`

###### **Arguments:**

* `<USER_ID>`
* `<VERSION>`



## `admin query users get-all-backups`

**Usage:** `admin query users get-all-backups <USER_ID> <VERSION>`

###### **Arguments:**

* `<USER_ID>`
* `<VERSION>`



## `admin query users get-room-backups`

**Usage:** `admin query users get-room-backups <USER_ID> <VERSION> <ROOM_ID>`

###### **Arguments:**

* `<USER_ID>`
* `<VERSION>`
* `<ROOM_ID>`



## `admin query users get-backup-session`

**Usage:** `admin query users get-backup-session <USER_ID> <VERSION> <ROOM_ID> <SESSION_ID>`

###### **Arguments:**

* `<USER_ID>`
* `<VERSION>`
* `<ROOM_ID>`
* `<SESSION_ID>`



## `admin query users get-shared-rooms`

**Usage:** `admin query users get-shared-rooms <USER_A> <USER_B>`

###### **Arguments:**

* `<USER_A>`
* `<USER_B>`



## `admin query resolver`

- resolver service

**Usage:** `admin query resolver <COMMAND>`

###### **Subcommands:**

* `destinations-cache` — Query the destinations cache
* `overrides-cache` — Query the overrides cache



## `admin query resolver destinations-cache`

Query the destinations cache

**Usage:** `admin query resolver destinations-cache [SERVER_NAME]`

###### **Arguments:**

* `<SERVER_NAME>`



## `admin query resolver overrides-cache`

Query the overrides cache

**Usage:** `admin query resolver overrides-cache [NAME]`

###### **Arguments:**

* `<NAME>`



## `admin query pusher`

- pusher service

**Usage:** `admin query pusher <COMMAND>`

###### **Subcommands:**

* `get-pushers` — - Returns all the pushers for the user



## `admin query pusher get-pushers`

- Returns all the pushers for the user

**Usage:** `admin query pusher get-pushers <USER_ID>`

###### **Arguments:**

* `<USER_ID>` — Full user ID



## `admin query short`

- short service

**Usage:** `admin query short <COMMAND>`

###### **Subcommands:**

* `short-event-id` —
* `short-room-id` —



## `admin query short short-event-id`

**Usage:** `admin query short short-event-id <EVENT_ID>`

###### **Arguments:**

* `<EVENT_ID>`



## `admin query short short-room-id`

**Usage:** `admin query short short-room-id <ROOM_ID>`

###### **Arguments:**

* `<ROOM_ID>`



## `admin query raw`

- raw service

**Usage:** `admin query raw <COMMAND>`

###### **Subcommands:**

* `raw-maps` — - List database maps
* `raw-get` — - Raw database query
* `raw-del` — - Raw database delete (for string keys)
* `raw-keys` — - Raw database keys iteration
* `raw-keys-sizes` — - Raw database key size breakdown
* `raw-keys-total` — - Raw database keys total bytes
* `raw-vals-sizes` — - Raw database values size breakdown
* `raw-vals-total` — - Raw database values total bytes
* `raw-iter` — - Raw database items iteration
* `raw-keys-from` — - Raw database keys iteration
* `raw-iter-from` — - Raw database items iteration
* `raw-count` — - Raw database record count
* `compact` — - Compact database



## `admin query raw raw-maps`

- List database maps

**Usage:** `admin query raw raw-maps`



## `admin query raw raw-get`

- Raw database query

**Usage:** `admin query raw raw-get <MAP> <KEY>`

###### **Arguments:**

* `<MAP>` — Map name
* `<KEY>` — Key



## `admin query raw raw-del`

- Raw database delete (for string keys)

**Usage:** `admin query raw raw-del <MAP> <KEY>`

###### **Arguments:**

* `<MAP>` — Map name
* `<KEY>` — Key



## `admin query raw raw-keys`

- Raw database keys iteration

**Usage:** `admin query raw raw-keys <MAP> [PREFIX]`

###### **Arguments:**

* `<MAP>` — Map name
* `<PREFIX>` — Key prefix



## `admin query raw raw-keys-sizes`

- Raw database key size breakdown

**Usage:** `admin query raw raw-keys-sizes [MAP] [PREFIX]`

###### **Arguments:**

* `<MAP>` — Map name
* `<PREFIX>` — Key prefix



## `admin query raw raw-keys-total`

- Raw database keys total bytes

**Usage:** `admin query raw raw-keys-total [MAP] [PREFIX]`

###### **Arguments:**

* `<MAP>` — Map name
* `<PREFIX>` — Key prefix



## `admin query raw raw-vals-sizes`

- Raw database values size breakdown

**Usage:** `admin query raw raw-vals-sizes [MAP] [PREFIX]`

###### **Arguments:**

* `<MAP>` — Map name
* `<PREFIX>` — Key prefix



## `admin query raw raw-vals-total`

- Raw database values total bytes

**Usage:** `admin query raw raw-vals-total [MAP] [PREFIX]`

###### **Arguments:**

* `<MAP>` — Map name
* `<PREFIX>` — Key prefix



## `admin query raw raw-iter`

- Raw database items iteration

**Usage:** `admin query raw raw-iter <MAP> [PREFIX]`

###### **Arguments:**

* `<MAP>` — Map name
* `<PREFIX>` — Key prefix



## `admin query raw raw-keys-from`

- Raw database keys iteration

**Usage:** `admin query raw raw-keys-from [OPTIONS] <MAP> <START>`

###### **Arguments:**

* `<MAP>` — Map name
* `<START>` — Lower-bound

###### **Options:**

* `-l`, `--limit <LIMIT>` — Limit



## `admin query raw raw-iter-from`

- Raw database items iteration

**Usage:** `admin query raw raw-iter-from [OPTIONS] <MAP> <START>`

###### **Arguments:**

* `<MAP>` — Map name
* `<START>` — Lower-bound

###### **Options:**

* `-l`, `--limit <LIMIT>` — Limit



## `admin query raw raw-count`

- Raw database record count

**Usage:** `admin query raw raw-count [MAP] [PREFIX]`

###### **Arguments:**

* `<MAP>` — Map name
* `<PREFIX>` — Key prefix



## `admin query raw compact`

- Compact database

**Usage:** `admin query raw compact [OPTIONS]`

###### **Options:**

* `-m`, `--map <MAP>`
* `--start <START>`
* `--stop <STOP>`
* `--from <FROM>`
* `--into <INTO>`
* `--parallelism <PARALLELISM>` — There is one compaction job per column; then this controls how many columns are compacted in parallel. If zero, one compaction job is still run at a time here, but in exclusive-mode blocking any other automatic compaction jobs until complete
* `--exhaustive`

  Default value: `false`
