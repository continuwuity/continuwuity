# Continuwuity Database Column Relationships

This document analyzes how the 89 database columns in Continuwuity relate to each other, showing the data flow and dependencies between tables.

## Core Identity Mapping System

### Event ID Management

The system uses a sophisticated event ID mapping system to optimize storage:

```
eventid_shorteventid ←→ shorteventid_eventid
         ↓                      ↓
   eventid_pduid         shorteventid_authchain
         ↓                      ↓
     pduid_pdu         shorteventid_shortstatehash
         ↓
eventid_outlierpdu
```

**Relationships:**

- `eventid_shorteventid` + `shorteventid_eventid`: Bidirectional mapping between full Matrix event IDs (48 bytes) and compact short IDs (8 bytes)
- `eventid_pduid`: Maps event IDs to PDU IDs (16-byte internal identifiers)
- `pduid_pdu`: Main event storage using PDU IDs as keys
- `eventid_outlierpdu`: Stores events not yet part of the timeline (outliers)
- `shorteventid_authchain`: Authorization chains using short event IDs
- `shorteventid_shortstatehash`: Links events to room state using short IDs

### Room ID Management

Similar optimization for room identifiers:

```
roomid_shortroomid
       ↓
   (used in keys for room-related tables)
```

### State Management

Complex state tracking with compression:

```
statekey_shortstatekey ←→ shortstatekey_statekey
         ↓
statehash_shortstatehash
         ↓
shortstatehash_statediff
         ↓
roomid_shortstatehash
```

**Relationships:**

- `statekey_shortstatekey` + `shortstatekey_statekey`: Bidirectional mapping for state keys
- `statehash_shortstatehash`: Maps full state hashes to 8-byte compressed versions
- `shortstatehash_statediff`: Stores state differences between versions
- `roomid_shortstatehash`: Current state hash for each room

## User and Authentication Flow

### User Authentication Chain

```
userid_password → token_userdeviceid ←→ userdeviceid_token
                         ↓
                 userdeviceid_metadata
                         ↓
              userdevicesessionid_uiaainfo
```

**Relationships:**

- `userid_password`: Stores user password hashes
- `token_userdeviceid` + `userdeviceid_token`: Bidirectional mapping between access tokens and devices
- `userdeviceid_metadata`: Device information (name, type, etc.)
- `userdevicesessionid_uiaainfo`: User-Interactive Authentication session data

### User Profile Data

```
userid_displayname
userid_avatarurl → userid_blurhash
useridprofilekey_value
```

**Relationships:**

- Profile data is stored separately per attribute
- `userid_blurhash` complements `userid_avatarurl` for progressive loading

### Token Management

```
openidtoken_expiresatuserid
logintoken_expiresatuserid  
tokenids
```

**Relationships:**

- Separate token types have separate expiration tracking
- `tokenids` manages token ID allocation

## Room Membership System

### Membership State Tracking

```
roomuserid_joined ←→ userroomid_joined
roomuserid_invitecount ←→ userroomid_invitestate
roomuserid_leftcount ←→ userroomid_leftstate
roomuserid_knockedcount ←→ userroomid_knockedstate
```

**Relationships:**

- Bidirectional indexes: room→user and user→room perspectives
- Count tables track membership transitions
- State tables store membership event data

### Room Counts and Metadata

```
roomid_joinedcount ← roomuserid_joined
roomid_invitedcount ← roomuserid_invitecount
roomuseroncejoinedids (historical tracking)
```

**Relationships:**

- Count tables are derived from individual membership records
- Historical tracking for users who ever joined

### Federation Integration

```
roomserverids ←→ serverroomids
roomid_inviteviaservers
```

**Relationships:**

- Bidirectional tracking of which servers participate in which rooms
- Via servers for invitation routing

## Cryptography and Security

### Device Key Management

```
userid_devicelistversion
         ↓
keyid_key ← userid_masterkeyid
         ↓     userid_selfsigningkeyid
keychangeid_userid ← userid_usersigningkeyid
         ↓
onetimekeyid_onetimekeys
         ↓
userid_lastonetimekeyupdate
```

**Relationships:**

- Device list versions track changes requiring key updates
- Different key types stored separately with references from user records
- Key changes trigger notifications
- One-time keys managed with update timestamps

### Key Backup System

```
backupid_algorithm
backupid_etag → backupkeyid_backup
```

**Relationships:**

- Backup metadata (algorithm, versioning) linked to actual backed-up keys

## Push Notifications and Read Tracking

### Push Infrastructure

```
senderkey_pusher ←→ pushkey_deviceid
```

**Relationships:**

- Bidirectional mapping between push keys and devices

### Read Receipt System

```
readreceiptid_readreceipt
roomuserid_privateread
roomuserid_lastprivatereadupdate
userroomid_highlightcount
userroomid_notificationcount
```

**Relationships:**

- Public read receipts vs private read markers
- Highlight/notification counts per user-room pair
- Update tracking for private reads

## Media and Content

### Media Storage

```
mediaid_file ←→ mediaid_user
url_previews
```

**Relationships:**

- File metadata linked to uploader tracking
- URL previews cached separately

## Sync and Timeline

### Sync Token Management

```
roomsynctoken_shortstatehash
lazyloadedids
```

**Relationships:**

- Sync tokens map to room state for efficient delta computation
- Lazy loading tracking for member events

### Event Relations

```
tofrom_relation
threadid_userids
referencedevents
softfailedeventids
```

**Relationships:**

- Event relations track replies, edits, reactions
- Thread participant tracking
- Referenced events and soft failures

## Federation and Server Management

### Server Discovery and Communication

```
servername_destination (cached)
servername_override (cached)
server_signingkeys
servername_educount
servercurrentevent_data
servernameevent_data
```

**Relationships:**

- Destination resolution with caching
- Server signing keys for federation
- EDU (Ephemeral Data Unit) counting
- Current and historical server events

## Account Data and Presence

### Account Data Storage

```
roomusertype_roomuserdataid → roomuserdataid_accountdata
userid_presenceid → presenceid_presence
```

**Relationships:**

- Account data indexed by room+user+type, pointing to actual data
- Presence data separated from user records with ID mapping

## Global Configuration

### Application Services

```
id_appserviceregistrations
```

### Global Settings

```
global
publicroomids
bannedroomids
disabledroomids
```

**Relationships:**

- Global server configuration
- Room access control lists

## Performance Optimizations

### Shared Cache Relationships

- `eventid_outlierpdu` and `pduid_pdu` share cache because they both store PDU data
- Related tables are grouped for memory efficiency

### Transaction Management

```
userdevicetxnid_response
todeviceid_events
```

**Relationships:**

- Transaction ID response caching
- To-device message queuing

## Data Flow Examples

### Sending a Message

1. `pduid_pdu` ← stores the PDU
2. `eventid_pduid` ← maps event ID to PDU ID
3. `eventid_shorteventid` ← creates short ID mapping
4. `shorteventid_shortstatehash` ← links to room state
5. `userroomid_notificationcount` ← updates notification counts
6. `readreceiptid_readreceipt` ← processes read receipts

### User Login

1. `userid_password` ← validates credentials
2. `userdeviceid_token` ← creates device token
3. `token_userdeviceid` ← creates reverse mapping
4. `userdeviceid_metadata` ← stores device info

### Room Join

1. `roomuserid_joined` ← records membership
2. `userroomid_joined` ← creates reverse index
3. `roomid_joinedcount` ← updates room count
4. `roomuseroncejoinedids` ← historical tracking
5. `roomserverids` ← federation tracking

This relational structure allows Continuwuity to efficiently handle Matrix protocol operations while maintaining data consistency and enabling fast lookups from multiple perspectives.
