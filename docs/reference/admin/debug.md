# `!admin debug`

- Commands for debugging things


## `!admin debug echo`

- Echo input of admin command

## `!admin debug get-auth-chain`

- Get the auth_chain of a PDU

## `!admin debug parse-pdu`

- Parse and print a PDU from a JSON

The PDU event is only checked for validity and is not added to the database.

This command needs a JSON blob provided in a Markdown code block below the command.

## `!admin debug get-pdu`

- Retrieve and print a PDU by EventID from the Continuwuity database

## `!admin debug get-short-pdu`

- Retrieve and print a PDU by PduId from the Continuwuity database

## `!admin debug get-remote-pdu`

- Attempts to retrieve a PDU from a remote server. **Does not** insert it into the database or persist it anywhere

## `!admin debug get-remote-pdu-list`

- Same as `get-remote-pdu` but accepts a codeblock newline delimited list of PDUs and a single server to fetch from

## `!admin debug get-room-state`

- Gets all the room state events for the specified room.

This is functionally equivalent to `GET /_matrix/client/v3/rooms/{roomid}/state`, except the admin command does *not* check if the sender user is allowed to see state events. This is done because it's implied that server admins here have database access and can see/get room info themselves anyways if they were malicious admins.

Of course the check is still done on the actual client API.

## `!admin debug get-signing-keys`

- Get and display signing keys from local cache or remote server

## `!admin debug get-verify-keys`

- Get and display signing keys from local cache or remote server

## `!admin debug ping`

- Sends a federation request to the remote server's `/_matrix/federation/v1/version` endpoint and measures the latency it took for the server to respond

## `!admin debug force-device-list-updates`

- Forces device lists for all local and remote users to be updated (as having new keys available)

## `!admin debug change-log-level`

- Change tracing log level/filter on the fly

This accepts the same format as the `log` config option.

## `!admin debug verify-json`

- Verify JSON signatures

This command needs a JSON blob provided in a Markdown code block below the command.

## `!admin debug verify-pdu`

- Verify PDU

This re-verifies a PDU existing in the database found by ID.

## `!admin debug first-pdu-in-room`

- Prints the very first PDU in the specified room (typically m.room.create)

## `!admin debug latest-pdu-in-room`

- Prints the latest ("last") PDU in the specified room (typically a message)

## `!admin debug force-set-room-state-from-server`

- Forcefully replaces the room state of our local copy of the specified room, with the copy (auth chain and room state events) the specified remote server says.

A common desire for room deletion is to simply "reset" our copy of the room. While this admin command is not a replacement for that, if you know you have split/broken room state and you know another server in the room that has the best/working room state, this command can let you use their room state. Such example is your server saying users are in a room, but other servers are saying they're not in the room in question.

This command will get the latest PDU in the room we know about, and request the room state at that point in time via `/_matrix/federation/v1/state/{roomId}`.

## `!admin debug resolve-true-destination`

- Runs a server name through Continuwuity's true destination resolution process

Useful for debugging well-known issues

## `!admin debug memory-stats`

- Print extended memory usage

Optional argument is a character mask (a sequence of characters in any order) which enable additional extended statistics. Known characters are "abdeglmx". For convenience, a '*' will enable everything.

## `!admin debug runtime-metrics`

- Print general tokio runtime metric totals

## `!admin debug runtime-interval`

- Print detailed tokio runtime metrics accumulated since last command invocation

## `!admin debug time`

- Print the current time

## `!admin debug list-dependencies`

- List dependencies

## `!admin debug database-stats`

- Get database statistics

## `!admin debug trim-memory`

- Trim memory usage

## `!admin debug database-files`

- List database files

## `!admin debug tester`

- Developer test stubs

### `!admin debug tester panic`

_(no description)_

### `!admin debug tester failure`

_(no description)_

### `!admin debug tester tester`

_(no description)_

### `!admin debug tester timer`

_(no description)_
