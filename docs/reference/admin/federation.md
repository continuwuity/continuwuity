# `!admin federation`

- Commands for managing federation


## `!admin federation incoming-federation`

- List all rooms we are currently handling an incoming pdu from

## `!admin federation disable-room`

- Disables incoming federation handling for a room

## `!admin federation enable-room`

- Enables incoming federation handling for a room again

## `!admin federation fetch-support-well-known`

- Fetch `/.well-known/matrix/support` from the specified server

Despite the name, this is not a federation endpoint and does not go through the federation / server resolution process as per-spec this is supposed to be served at the server_name.

Respecting homeservers put this file here for listing administration, moderation, and security inquiries. This command provides a way to easily fetch that information.

## `!admin federation remote-user-in-rooms`

- Lists all the rooms we share/track with the specified *remote* user
