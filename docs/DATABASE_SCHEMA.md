# Continuwuity Database Schema Documentation

Continuwuity is a Matrix protocol implementation using RocksDB as its storage backend. The database is organized into column families (called "Maps" in the codebase), each serving specific purposes in the Matrix homeserver functionality.

| Table Name | Access Pattern | Key Size | Value Size | Description |
|------------|---------------|----------|------------|-------------|
| `alias_roomid` | RANDOM_SMALL | - | - | Maps room alias to room ID |
| `alias_userid` | RANDOM_SMALL | - | - | Maps room alias to user ID |
| `aliasid_alias` | RANDOM_SMALL | - | - | Maps alias ID to alias string |
| `backupid_algorithm` | RANDOM_SMALL | - | - | Key backup algorithms |
| `backupid_etag` | RANDOM_SMALL | - | - | Key backup ETags |
| `backupkeyid_backup` | RANDOM_SMALL | - | - | Backed up keys |
| `bannedroomids` | RANDOM_SMALL | - | - | Set of banned room IDs |
| `disabledroomids` | RANDOM_SMALL | - | - | Set of disabled room IDs |
| `eventid_outlierpdu` | RANDOM | 48 bytes | 1488 bytes | Outlier PDUs (shared cache with pduid_pdu) |
| `eventid_pduid` | RANDOM | 48 bytes | 16 bytes | Event ID to PDU ID mapping |
| `eventid_shorteventid` | RANDOM | 48 bytes | 8 bytes | Event ID to short event ID |
| `global` | RANDOM_SMALL | - | - | Global server configuration |
| `id_appserviceregistrations` | RANDOM_SMALL | - | - | Application service registrations |
| `keychangeid_userid` | RANDOM | - | - | Key change notifications |
| `keyid_key` | RANDOM_SMALL | - | - | Cryptographic keys |
| `lazyloadedids` | RANDOM_SMALL | - | - | Lazy-loaded member events |
| `mediaid_file` | RANDOM_SMALL | - | - | Media file metadata |
| `mediaid_user` | RANDOM_SMALL | - | - | Media uploader tracking |
| `onetimekeyid_onetimekeys` | RANDOM_SMALL | - | - | One-time keys |
| `pduid_pdu` | SEQUENTIAL | 16 bytes | 1520 bytes | Main PDU storage (shared cache with eventid_outlierpdu) |
| `publicroomids` | RANDOM_SMALL | - | - | Public room IDs |
| `pushkey_deviceid` | RANDOM_SMALL | - | - | Push key to device mapping |
| `presenceid_presence` | SEQUENTIAL_SMALL | - | - | User presence data |
| `readreceiptid_readreceipt` | RANDOM | - | - | Read receipts |
| `referencedevents` | RANDOM | - | - | Referenced events |
| `roomid_invitedcount` | RANDOM_SMALL | - | - | Room invited user count |
| `roomid_inviteviaservers` | RANDOM_SMALL | - | - | Room invite via servers |
| `roomid_joinedcount` | RANDOM_SMALL | - | - | Room joined user count |
| `roomid_pduleaves` | RANDOM_SMALL | - | - | PDU leaves per room |
| `roomid_shortroomid` | RANDOM_SMALL | - | 8 bytes | Room ID to short room ID |
| `roomid_shortstatehash` | RANDOM_SMALL | - | 8 bytes | Room ID to state hash |
| `roomserverids` | RANDOM_SMALL | - | - | Server IDs per room |
| `roomsynctoken_shortstatehash` | SEQUENTIAL | - | 8 bytes | Sync token to state hash (special compression) |
| `roomuserdataid_accountdata` | RANDOM_SMALL | - | - | Room account data |
| `roomuserid_invitecount` | RANDOM_SMALL | - | 8 bytes | Room-user invite count |
| `roomuserid_joined` | RANDOM_SMALL | - | - | Room-user joined status |
| `roomuserid_lastprivatereadupdate` | RANDOM_SMALL | - | - | Last private read update |
| `roomuserid_leftcount` | RANDOM | - | 8 bytes | Room-user leave count |
| `roomuserid_knockedcount` | RANDOM_SMALL | - | 8 bytes | Room-user knock count |
| `roomuserid_privateread` | RANDOM_SMALL | - | - | Private read markers |
| `roomuseroncejoinedids` | RANDOM | - | - | Users who ever joined |
| `roomusertype_roomuserdataid` | RANDOM_SMALL | - | - | Account data type mapping |
| `senderkey_pusher` | RANDOM_SMALL | - | - | Push notification senders |
| `server_signingkeys` | RANDOM | - | - | Server signing keys |
| `servercurrentevent_data` | RANDOM_SMALL | - | - | Current server events |
| `servername_destination` | RANDOM_SMALL_CACHE | - | - | Server destinations (cached) |
| `servername_educount` | RANDOM_SMALL | - | - | EDU counters |
| `servername_override` | RANDOM_SMALL_CACHE | - | - | Server name overrides (cached) |
| `servernameevent_data` | RANDOM | - | 128 bytes | Server event data |
| `serverroomids` | RANDOM_SMALL | - | - | Rooms per server |
| `shorteventid_authchain` | SEQUENTIAL | 8 bytes | - | Event authorization chains |
| `shorteventid_eventid` | SEQUENTIAL_SMALL | 8 bytes | 48 bytes | Short event ID to event ID |
| `shorteventid_shortstatehash` | SEQUENTIAL | 8 bytes | 8 bytes | Event to state hash mapping |
| `shortstatehash_statediff` | SEQUENTIAL_SMALL | 8 bytes | - | State differences |
| `shortstatekey_statekey` | RANDOM_SMALL | 8 bytes | 1016 bytes | Short state key to state key |
| `softfailedeventids` | RANDOM_SMALL | 48 bytes | - | Soft-failed events |
| `statehash_shortstatehash` | RANDOM | - | 8 bytes | State hash to short hash |
| `statekey_shortstatekey` | RANDOM | 1016 bytes | 8 bytes | State key to short key |
| `threadid_userids` | SEQUENTIAL_SMALL | - | - | Thread participants |
| `todeviceid_events` | RANDOM | - | - | To-device messages |
| `tofrom_relation` | RANDOM_SMALL | 8 bytes | 8 bytes | Event relations |
| `token_userdeviceid` | RANDOM_SMALL | - | - | Token to device mapping |
| `tokenids` | RANDOM | - | - | Token ID management |
| `url_previews` | RANDOM | - | - | URL preview cache |
| `userdeviceid_metadata` | RANDOM_SMALL | - | - | Device metadata |
| `userdeviceid_token` | RANDOM_SMALL | - | - | Device tokens |
| `userdevicesessionid_uiaainfo` | RANDOM_SMALL | - | - | UIAA session info |
| `userdevicetxnid_response` | RANDOM_SMALL | - | - | Transaction responses |
| `userfilterid_filter` | RANDOM_SMALL | - | - | User sync filters |
| `userid_avatarurl` | RANDOM_SMALL | - | - | User avatar URLs |
| `userid_blurhash` | RANDOM_SMALL | - | - | Avatar blurhashes |
| `userid_devicelistversion` | RANDOM_SMALL | - | - | Device list versions |
| `userid_displayname` | RANDOM_SMALL | - | - | User display names |
| `userid_lastonetimekeyupdate` | RANDOM_SMALL | - | - | Last OTK update time |
| `userid_masterkeyid` | RANDOM_SMALL | - | - | Master signing keys |
| `userid_password` | RANDOM | - | - | Password hashes |
| `userid_presenceid` | RANDOM_SMALL | - | - | User presence mapping |
| `userid_selfsigningkeyid` | RANDOM_SMALL | - | - | Self-signing keys |
| `userid_usersigningkeyid` | RANDOM_SMALL | - | - | User-signing keys |
| `useridprofilekey_value` | RANDOM_SMALL | - | - | Custom profile fields |
| `openidtoken_expiresatuserid` | RANDOM_SMALL | - | - | OpenID tokens |
| `logintoken_expiresatuserid` | RANDOM_SMALL | - | - | Login tokens |
| `userroomid_highlightcount` | RANDOM | - | - | Highlight counts |
| `userroomid_invitestate` | RANDOM_SMALL | - | - | User invite states |
| `userroomid_joined` | RANDOM | - | - | User joined rooms |
| `userroomid_leftstate` | RANDOM | - | - | User leave states |
| `userroomid_knockedstate` | RANDOM_SMALL | - | - | User knock states |
| `userroomid_notificationcount` | RANDOM | - | - | Notification counts |

## Access Pattern Definitions

### RANDOM

- Large datasets with random updates across keyspace
- Compaction priority: OldestSmallestSeqFirst
- Write buffer: 32MB
- Cache shards: 128
- Compression: Zstd level -3
- Bottommost compression: level 2

### SEQUENTIAL  

- Large datasets with append-heavy updates
- Compaction priority: OldestLargestSeqFirst
- Write buffer: 64MB
- Level size: 32MB
- File size: 2MB
- Compression: Zstd level -2

### RANDOM_SMALL

- Small datasets with random updates
- Compaction style: Universal
- Write buffer: 16MB
- Level size: 512KB
- File size: 128KB
- Block size: 512 bytes
- Compression: Zstd level -4

### SEQUENTIAL_SMALL

- Small datasets with sequential updates
- Compaction style: Universal
- Write buffer: 16MB
- Level size: 1MB
- File size: 512KB
- Compression: Zstd level -4

### RANDOM_SMALL_CACHE

- Small persistent caches with TTL
- Compaction style: FIFO
- Size limit: 64MB
- TTL: 14 days
- Unique cache allocation

## Special Configurations

### Shared Cache Tables

- `eventid_outlierpdu` and `pduid_pdu` share cache pool
- Optimizes memory usage for related event data

### High-Performance Tables

- `roomsynctoken_shortstatehash`: Special compression settings for sync performance
- `pduid_pdu`: Large block size (2KB) for efficient event storage
- `eventid_outlierpdu`: Optimized for outlier PDU handling

### Cache-Only Tables

- `servername_destination`: FIFO cache for server resolution
- `servername_override`: FIFO cache for server overrides

## Data Types and Sizes

### Event IDs

- Full event IDs: 48 bytes (Matrix event ID format)
- Short event IDs: 8 bytes (internal optimization)

### Room IDs  

- Full room IDs: Variable length Matrix room ID
- Short room IDs: 8 bytes (internal optimization)

### PDU Data

- PDU ID: 16 bytes
- PDU content: ~1520 bytes average
- Outlier PDUs: ~1488 bytes average

### State Data

- State keys: Up to 1016 bytes
- Short state keys: 8 bytes
- State hashes: 8 bytes (shortened)

This technical reference shows how Continuwuity optimizes storage for different types of Matrix data, using appropriate RocksDB configurations for each access pattern.

## Database Architecture

## Column Families (Maps)

### Room Management

#### Room Aliases

- **`alias_roomid`**: Maps room alias to room ID
- **`alias_userid`**: Maps room alias to user ID (for alias management)
- **`aliasid_alias`**: Maps alias ID to actual alias string

#### Room Metadata

- **`roomid_shortroomid`**: Maps room ID to short room ID (8-byte identifier)
- **`roomid_shortstatehash`**: Maps room ID to current state hash
- **`roomid_pduleaves`**: Tracks PDU leaves for each room
- **`roomid_invitedcount`**: Count of invited users per room
- **`roomid_joinedcount`**: Count of joined users per room
- **`roomid_inviteviaservers`**: Via servers for room invites
- **`publicroomids`**: Set of public room IDs
- **`bannedroomids`**: Set of banned room IDs
- **`disabledroomids`**: Set of disabled room IDs

#### Room State

- **`shortstatehash_statediff`**: State differences between state hashes
- **`statehash_shortstatehash`**: Maps full state hash to short state hash (8-byte)
- **`statekey_shortstatekey`**: Maps state key to short state key (8-byte)
- **`shortstatekey_statekey`**: Reverse mapping from short state key to full state key
- **`roomsynctoken_shortstatehash`**: Maps room sync tokens to state hashes

### Events and Timeline

#### Event Storage

- **`eventid_pduid`**: Maps event ID to PDU ID (16-byte identifier)
- **`eventid_shorteventid`**: Maps event ID to short event ID (8-byte)
- **`eventid_outlierpdu`**: Stores outlier PDUs (events not yet in timeline)
- **`pduid_pdu`**: Main PDU storage (PDU ID to PDU data)
- **`shorteventid_eventid`**: Reverse mapping from short event ID to full event ID
- **`shorteventid_authchain`**: Authorization chains for events
- **`shorteventid_shortstatehash`**: Maps events to their state hashes

#### Event Relationships

- **`tofrom_relation`**: Event relations (replies, edits, reactions)
- **`threadid_userids`**: Thread participants tracking
- **`referencedevents`**: Referenced events tracking
- **`softfailedeventids`**: Events that soft-failed state resolution

### User Management

#### User Identity

- **`userid_displayname`**: User display names
- **`userid_avatarurl`**: User avatar URLs
- **`userid_blurhash`**: Avatar blurhash values
- **`userid_password`**: Password hashes
- **`useridprofilekey_value`**: Custom profile fields

#### User Devices and Sessions

- **`userdeviceid_metadata`**: Device metadata (name, type, etc.)
- **`userdeviceid_token`**: Device access tokens
- **`token_userdeviceid`**: Reverse token to device mapping
- **`userdevicesessionid_uiaainfo`**: User-Interactive Auth session data
- **`userdevicetxnid_response`**: Transaction ID to response caching

#### User Preferences

- **`userfilterid_filter`**: Sync filter definitions
- **`lazyloadedids`**: Lazy-loaded member event tracking

### Cryptography and Security

#### Device Keys

- **`keyid_key`**: Cryptographic keys storage
- **`userid_devicelistversion`**: Device list versions for users
- **`userid_lastonetimekeyupdate`**: Last one-time key update timestamps
- **`onetimekeyid_onetimekeys`**: One-time keys storage

#### Cross-Signing

- **`userid_masterkeyid`**: Master signing keys
- **`userid_selfsigningkeyid`**: Self-signing keys
- **`userid_usersigningkeyid`**: User-signing keys
- **`keychangeid_userid`**: Key change notifications

#### Key Backups

- **`backupid_algorithm`**: Backup algorithm information
- **`backupid_etag`**: Backup ETags for versioning
- **`backupkeyid_backup`**: Backed up keys

### Room Membership

#### Membership States

- **`roomuserid_joined`**: Current joined room members
- **`roomuserid_invitecount`**: Invite counts per room-user
- **`roomuserid_leftcount`**: Leave counts per room-user
- **`roomuserid_knockedcount`**: Knock counts per room-user
- **`roomuseroncejoinedids`**: Users who have ever joined rooms

#### Membership Events

- **`userroomid_joined`**: User's joined rooms
- **`userroomid_invitestate`**: Invite state events
- **`userroomid_leftstate`**: Leave state events
- **`userroomid_knockedstate`**: Knock state events

### Push Notifications and Read Receipts

#### Push Infrastructure

- **`senderkey_pusher`**: Push notification endpoints
- **`pushkey_deviceid`**: Push key to device mappings

#### Read Tracking

- **`readreceiptid_readreceipt`**: Read receipt storage
- **`roomuserid_privateread`**: Private read markers
- **`roomuserid_lastprivatereadupdate`**: Last private read updates
- **`userroomid_highlightcount`**: Highlight/mention counts
- **`userroomid_notificationcount`**: Notification counts per room

### Media and Content

#### Media Storage

- **`mediaid_file`**: Media file metadata
- **`mediaid_user`**: Media uploader tracking
- **`url_previews`**: URL preview cache

### Federation and Server-to-Server

#### Server Management

- **`server_signingkeys`**: Server signing keys
- **`servername_destination`**: Server destination resolution
- **`servername_educount`**: Ephemeral Data Unit counters
- **`servername_override`**: Server name overrides for federation
- **`servernameevent_data`**: Server event data
- **`roomserverids`**: Servers participating in rooms
- **`serverroomids`**: Rooms per server
- **`servercurrentevent_data`**: Current server event state

### Application Services

- **`id_appserviceregistrations`**: Application service registrations

### Account Data and Presence

#### Account Data

- **`roomuserdataid_accountdata`**: Room-specific account data
- **`roomusertype_roomuserdataid`**: Account data type mappings

#### Presence

- **`presenceid_presence`**: User presence information
- **`userid_presenceid`**: User to presence ID mapping

### To-Device Messages

- **`todeviceid_events`**: Direct device-to-device messages

### Authentication Tokens

- **`openidtoken_expiresatuserid`**: OpenID Connect tokens
- **`logintoken_expiresatuserid`**: Login tokens
- **`tokenids`**: Token ID management

### Global Configuration

- **`global`**: Global server settings and state

## Key Design Patterns

### Short Identifiers

Many tables use "short" versions of identifiers (8-byte integers) to reduce storage overhead:

- `shortroomid` for room IDs
- `shorteventid` for event IDs
- `shortstatekey` for state keys
- `shortstatehash` for state hashes

### Composite Keys

Key naming follows a pattern of `{primary}_{secondary}` to create efficient lookups:

- `roomuserid_*` for room-user relationships
- `userroomid_*` for user-room relationships
- `eventid_*` for event-related data

### Performance Optimizations

- **Cache sharing**: Related tables share cache pools (e.g., `eventid_outlierpdu` and `pduid_pdu`)
- **Access patterns**: Tables are optimized for their specific usage (RANDOM vs SEQUENTIAL)
- **Compression**: Different compression levels based on data characteristics
- **Block sizes**: Tuned based on expected key/value sizes

## Storage Efficiency

The schema is designed for efficiency in a Matrix homeserver context:

- Large event data uses sequential storage patterns
- Lookup tables use random access patterns
- Small metadata uses compressed storage
- Caching is strategically shared between related data

This design allows Continuwuity to efficiently handle the complex relationships and high-volume data typical in Matrix federation while maintaining good performance characteristics for both reads and writes.

## Column Relationships and Data Flow

### Core Event Storage Chain

The heart of the Matrix homeserver is event storage, which uses several interconnected tables:

- `eventid_shorteventid` ↔ `shorteventid_eventid`: Bidirectional mapping for event ID compression (48 bytes → 8 bytes)
- `eventid_pduid`: Maps Matrix event IDs to internal PDU IDs (16 bytes)
- `pduid_pdu`: Main event storage using PDU IDs as keys (shares cache with `eventid_outlierpdu`)
- `eventid_outlierpdu`: Stores events not yet integrated into the timeline
- `shorteventid_authchain`: Authorization chains using compressed event IDs
- `shorteventid_shortstatehash`: Links events to room state snapshots

### Room State Management

Room state is tracked through multiple interconnected tables:

- `statekey_shortstatekey` ↔ `shortstatekey_statekey`: Bidirectional state key compression
- `statehash_shortstatehash`: Compresses state hashes from full size to 8 bytes
- `shortstatehash_statediff`: Stores incremental state changes
- `roomid_shortstatehash`: Current state hash for each room
- `roomsynctoken_shortstatehash`: Maps sync tokens to state for efficient delta sync

### User Identity and Authentication

User management involves several related tables:

- `userid_password` → authentication base
- `token_userdeviceid` ↔ `userdeviceid_token`: Bidirectional token↔device mapping
- `userdeviceid_metadata`: Device information storage
- `userid_displayname`, `userid_avatarurl`, `userid_blurhash`: Profile data
- `openidtoken_expiresatuserid`, `logintoken_expiresatuserid`: Token management

### Room Membership Tracking

Membership uses bidirectional indexes for efficient queries:

- `roomuserid_joined` ↔ `userroomid_joined`: Current membership from both perspectives
- `roomuserid_invitecount` ↔ `userroomid_invitestate`: Invitation tracking
- `roomuserid_leftcount` ↔ `userroomid_leftstate`: Leave event tracking
- `roomid_joinedcount`, `roomid_invitedcount`: Aggregate room statistics
- `roomuseroncejoinedids`: Historical membership tracking

### Cryptography and Security Chain

End-to-end encryption involves coordinated key management:

- `userid_devicelistversion`: Tracks when device lists change
- `keyid_key`: Stores actual cryptographic keys
- `userid_masterkeyid`, `userid_selfsigningkeyid`, `userid_usersigningkeyid`: Cross-signing keys
- `onetimekeyid_onetimekeys` → `userid_lastonetimekeyupdate`: One-time key lifecycle
- `keychangeid_userid`: Key change notifications
- `backupid_algorithm`, `backupid_etag` → `backupkeyid_backup`: Key backup system

### Federation and Server Communication

Server-to-server communication requires coordinated tracking:

- `roomserverids` ↔ `serverroomids`: Bidirectional room↔server participation
- `servername_destination`, `servername_override`: Server resolution (both cached)
- `server_signingkeys`: Federation authentication
- `servername_educount`: Ephemeral data unit tracking
- `servernameevent_data`, `servercurrentevent_data`: Server event state

### Read Tracking and Notifications

Message read tracking involves multiple coordinated updates:

- `readreceiptid_readreceipt`: Public read receipts
- `roomuserid_privateread`, `roomuserid_lastprivatereadupdate`: Private read markers
- `userroomid_highlightcount`, `userroomid_notificationcount`: Per-room notification counts
- `senderkey_pusher` ↔ `pushkey_deviceid`: Push notification routing

### Account Data and Preferences

User preferences and account data use a two-level structure:

- `roomusertype_roomuserdataid` → `roomuserdataid_accountdata`: Type index points to actual data
- `userid_presenceid` → `presenceid_presence`: Presence data separation
- `userfilterid_filter`: Sync filter definitions
- `lazyloadedids`: Lazy loading state tracking

This interconnected design allows Continuwuity to efficiently handle Matrix protocol operations while maintaining data consistency and enabling fast lookups from multiple perspectives.
