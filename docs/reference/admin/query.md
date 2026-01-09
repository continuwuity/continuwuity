# `!admin query`

- Low-level queries for database getters and iterators


## `!admin query account-data`

- account_data.rs iterators and getters

### `!admin query account-data changes-since`

- Returns all changes to the account data that happened after `since`

### `!admin query account-data account-data-get`

- Searches the account data for a specific kind

## `!admin query appservice`

- appservice.rs iterators and getters

### `!admin query appservice get-registration`

- Gets the appservice registration info/details from the ID as a string

### `!admin query appservice all`

- Gets all appservice registrations with their ID and registration info

## `!admin query presence`

- presence.rs iterators and getters

### `!admin query presence get-presence`

- Returns the latest presence event for the given user

### `!admin query presence presence-since`

- Iterator of the most recent presence updates that happened after the event with id `since`

## `!admin query room-alias`

- rooms/alias.rs iterators and getters

### `!admin query room-alias resolve-local-alias`

_(no description)_

### `!admin query room-alias local-aliases-for-room`

- Iterator of all our local room aliases for the room ID

### `!admin query room-alias all-local-aliases`

- Iterator of all our local aliases in our database with their room IDs

## `!admin query room-state-cache`

- rooms/state_cache iterators and getters

### `!admin query room-state-cache server-in-room`

_(no description)_

### `!admin query room-state-cache room-servers`

_(no description)_

### `!admin query room-state-cache server-rooms`

_(no description)_

### `!admin query room-state-cache room-members`

_(no description)_

### `!admin query room-state-cache local-users-in-room`

_(no description)_

### `!admin query room-state-cache active-local-users-in-room`

_(no description)_

### `!admin query room-state-cache room-joined-count`

_(no description)_

### `!admin query room-state-cache room-invited-count`

_(no description)_

### `!admin query room-state-cache room-user-once-joined`

_(no description)_

### `!admin query room-state-cache room-members-invited`

_(no description)_

### `!admin query room-state-cache get-invite-count`

_(no description)_

### `!admin query room-state-cache get-left-count`

_(no description)_

### `!admin query room-state-cache rooms-joined`

_(no description)_

### `!admin query room-state-cache rooms-left`

_(no description)_

### `!admin query room-state-cache rooms-invited`

_(no description)_

### `!admin query room-state-cache invite-state`

_(no description)_

## `!admin query room-timeline`

- rooms/timeline iterators and getters

### `!admin query room-timeline pdus`

_(no description)_

### `!admin query room-timeline last`

_(no description)_

## `!admin query globals`

- globals.rs iterators and getters

### `!admin query globals database-version`

_(no description)_

### `!admin query globals current-count`

_(no description)_

### `!admin query globals last-check-for-announcements-id`

_(no description)_

### `!admin query globals signing-keys-for`

- This returns an empty `Ok(BTreeMap<..>)` when there are no keys found for the server

## `!admin query sending`

- sending.rs iterators and getters

### `!admin query sending active-requests`

- Queries database for all `servercurrentevent_data`

### `!admin query sending active-requests-for`

- Queries database for `servercurrentevent_data` but for a specific destination

This command takes only *one* format of these arguments:

appservice_id server_name user_id AND push_key

See src/service/sending/mod.rs for the definition of the `Destination` enum

### `!admin query sending queued-requests`

- Queries database for `servernameevent_data` which are the queued up requests that will eventually be sent

This command takes only *one* format of these arguments:

appservice_id server_name user_id AND push_key

See src/service/sending/mod.rs for the definition of the `Destination` enum

### `!admin query sending get-latest-edu-count`

_(no description)_

## `!admin query users`

- users.rs iterators and getters

### `!admin query users count-users`

_(no description)_

### `!admin query users iter-users`

_(no description)_

### `!admin query users iter-users2`

_(no description)_

### `!admin query users password-hash`

_(no description)_

### `!admin query users list-devices`

_(no description)_

### `!admin query users list-devices-metadata`

_(no description)_

### `!admin query users get-device-metadata`

_(no description)_

### `!admin query users get-devices-version`

_(no description)_

### `!admin query users count-one-time-keys`

_(no description)_

### `!admin query users get-device-keys`

_(no description)_

### `!admin query users get-user-signing-key`

_(no description)_

### `!admin query users get-master-key`

_(no description)_

### `!admin query users get-to-device-events`

_(no description)_

### `!admin query users get-latest-backup`

_(no description)_

### `!admin query users get-latest-backup-version`

_(no description)_

### `!admin query users get-backup-algorithm`

_(no description)_

### `!admin query users get-all-backups`

_(no description)_

### `!admin query users get-room-backups`

_(no description)_

### `!admin query users get-backup-session`

_(no description)_

### `!admin query users get-shared-rooms`

_(no description)_

## `!admin query resolver`

- resolver service

### `!admin query resolver destinations-cache`

Query the destinations cache

### `!admin query resolver overrides-cache`

Query the overrides cache

## `!admin query pusher`

- pusher service

### `!admin query pusher get-pushers`

- Returns all the pushers for the user

## `!admin query short`

- short service

### `!admin query short short-event-id`

_(no description)_

### `!admin query short short-room-id`

_(no description)_

## `!admin query raw`

- raw service

### `!admin query raw raw-maps`

- List database maps

### `!admin query raw raw-get`

- Raw database query

### `!admin query raw raw-del`

- Raw database delete (for string keys)

### `!admin query raw raw-keys`

- Raw database keys iteration

### `!admin query raw raw-keys-sizes`

- Raw database key size breakdown

### `!admin query raw raw-keys-total`

- Raw database keys total bytes

### `!admin query raw raw-vals-sizes`

- Raw database values size breakdown

### `!admin query raw raw-vals-total`

- Raw database values total bytes

### `!admin query raw raw-iter`

- Raw database items iteration

### `!admin query raw raw-keys-from`

- Raw database keys iteration

### `!admin query raw raw-iter-from`

- Raw database items iteration

### `!admin query raw raw-count`

- Raw database record count

### `!admin query raw compact`

- Compact database
