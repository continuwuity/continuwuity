# Continuwuity Database Mermaid Diagrams

This document contains visual representations of the Continuwuity database schema using Mermaid diagrams.

## 1. Core Event Storage Architecture

```mermaid
graph TD
    A[Matrix Event ID<br/>48 bytes] --> B[eventid_shorteventid]
    B --> C[Short Event ID<br/>8 bytes]
    C --> D[shorteventid_eventid]
    D --> A
    
    A --> E[eventid_pduid]
    E --> F[PDU ID<br/>16 bytes]
    F --> G[pduid_pdu<br/>Main Event Storage]
    
    A --> H[eventid_outlierpdu<br/>Outlier Events]
    
    C --> I[shorteventid_authchain<br/>Authorization Chains]
    C --> J[shorteventid_shortstatehash<br/>Event → State Mapping]
    
    G -.->|Shared Cache| H
    
    style G fill:#e1f5fe
    style H fill:#e1f5fe
    style A fill:#fff3e0
    style C fill:#f3e5f5
```

## 2. Room State Management System

```mermaid
graph TD
    A[Room State Key] --> B[statekey_shortstatekey]
    B --> C[Short State Key<br/>8 bytes]
    C --> D[shortstatekey_statekey]
    D --> A
    
    E[Full State Hash] --> F[statehash_shortstatehash]
    F --> G[Short State Hash<br/>8 bytes]
    
    G --> H[shortstatehash_statediff<br/>State Differences]
    G --> I[roomid_shortstatehash<br/>Current Room State]
    G --> J[roomsynctoken_shortstatehash<br/>Sync Token Mapping]
    
    K[Room ID] --> I
    L[Sync Token] --> J
    
    style G fill:#e8f5e8
    style I fill:#fff3e0
    style J fill:#f0f4ff
```

## 3. User Authentication and Identity Flow

```mermaid
graph TD
    A[User ID] --> B[userid_password<br/>Password Hashes]
    
    A --> C[userid_displayname]
    A --> D[userid_avatarurl]
    D --> E[userid_blurhash]
    A --> F[useridprofilekey_value<br/>Custom Profile]
    
    G[Access Token] --> H[token_userdeviceid]
    H --> I[User + Device ID]
    I --> J[userdeviceid_token]
    J --> G
    
    I --> K[userdeviceid_metadata<br/>Device Info]
    I --> L[userdevicesessionid_uiaainfo<br/>Auth Sessions]
    I --> M[userdevicetxnid_response<br/>Transaction Cache]
    
    N[OpenID Token] --> O[openidtoken_expiresatuserid]
    P[Login Token] --> Q[logintoken_expiresatuserid]
    
    style H fill:#e1f5fe
    style J fill:#e1f5fe
    style B fill:#ffebee
```

## 4. Room Membership Bidirectional System

```mermaid
graph TD
    A[Room ID + User ID] --> B[roomuserid_joined<br/>Room → User View]
    C[User ID + Room ID] --> D[userroomid_joined<br/>User → Room View]
    
    B -.->|Bidirectional| D
    
    A --> E[roomuserid_invitecount]
    C --> F[userroomid_invitestate]
    E -.->|Related| F
    
    A --> G[roomuserid_leftcount]
    C --> H[userroomid_leftstate]
    G -.->|Related| H
    
    A --> I[roomuserid_knockedcount]
    C --> J[userroomid_knockedstate]
    I -.->|Related| J
    
    K[Room ID] --> L[roomid_joinedcount<br/>Total Joined]
    K --> M[roomid_invitedcount<br/>Total Invited]
    
    N[Historical] --> O[roomuseroncejoinedids<br/>Ever Joined Tracking]
    
    style B fill:#e8f5e8
    style D fill:#e8f5e8
    style L fill:#fff3e0
    style M fill:#fff3e0
```

## 5. Cryptography and Key Management Chain

```mermaid
graph TD
    A[User ID] --> B[userid_devicelistversion<br/>Device List Changes]
    
    A --> C[userid_masterkeyid<br/>Master Signing Key]
    A --> D[userid_selfsigningkeyid<br/>Self Signing Key]
    A --> E[userid_usersigningkeyid<br/>User Signing Key]
    
    F[Key ID] --> G[keyid_key<br/>Actual Keys]
    
    C --> G
    D --> G
    E --> G
    
    H[Key Change ID] --> I[keychangeid_userid<br/>Change Notifications]
    
    J[One-Time Key ID] --> K[onetimekeyid_onetimekeys<br/>OTK Storage]
    A --> L[userid_lastonetimekeyupdate<br/>Last OTK Update]
    
    M[Backup ID] --> N[backupid_algorithm<br/>Backup Algorithm]
    M --> O[backupid_etag<br/>Backup Versioning]
    P[Backup Key ID] --> Q[backupkeyid_backup<br/>Backed Up Keys]
    
    style G fill:#e1f5fe
    style I fill:#fff3e0
    style K fill:#f3e5f5
    style Q fill:#e8f5e8
```

## 6. Federation and Server Communication

```mermaid
graph TD
    A[Server Name] --> B[servername_destination<br/>Cached Destinations]
    A --> C[servername_override<br/>Cached Overrides]
    A --> D[server_signingkeys<br/>Federation Keys]
    A --> E[servername_educount<br/>EDU Counters]
    
    F[Server + Event] --> G[servernameevent_data<br/>Server Events]
    H[Server Current] --> I[servercurrentevent_data<br/>Current State]
    
    J[Room ID] --> K[roomserverids<br/>Room → Servers]
    L[Server Name] --> M[serverroomids<br/>Server → Rooms]
    
    K -.->|Bidirectional| M
    
    N[Room ID] --> O[roomid_inviteviaservers<br/>Invitation Routing]
    
    style B fill:#e1f5fe
    style C fill:#e1f5fe
    style K fill:#e8f5e8
    style M fill:#e8f5e8
```

## 7. Push Notifications and Read Tracking

```mermaid
graph TD
    A[Sender Key] --> B[senderkey_pusher<br/>Push Endpoints]
    C[Push Key] --> D[pushkey_deviceid<br/>Device Mapping]
    
    B -.->|Related| D
    
    E[Read Receipt ID] --> F[readreceiptid_readreceipt<br/>Public Receipts]
    
    G[Room + User] --> H[roomuserid_privateread<br/>Private Read Markers]
    G --> I[roomuserid_lastprivatereadupdate<br/>Update Timestamps]
    
    J[User + Room] --> K[userroomid_highlightcount<br/>Mention Count]
    J --> L[userroomid_notificationcount<br/>Notification Count]
    
    style F fill:#e8f5e8
    style H fill:#f3e5f5
    style K fill:#fff3e0
    style L fill:#fff3e0
```

## 8. Media and Content Management

```mermaid
graph TD
    A[Media ID] --> B[mediaid_file<br/>File Metadata]
    A --> C[mediaid_user<br/>Uploader Tracking]
    
    B -.->|Related| C
    
    D[URL] --> E[url_previews<br/>Preview Cache]
    
    F[User ID] --> G[userfilterid_filter<br/>Sync Filters]
    H[Lazy Load] --> I[lazyloadedids<br/>Member Event Tracking]
    
    style B fill:#e1f5fe
    style C fill:#e1f5fe
    style E fill:#f0f4ff
```

## 9. Account Data and Presence System

```mermaid
graph TD
    A[Room + User + Type] --> B[roomusertype_roomuserdataid<br/>Account Data Index]
    B --> C[Room User Data ID]
    C --> D[roomuserdataid_accountdata<br/>Actual Account Data]
    
    E[User ID] --> F[userid_presenceid<br/>Presence Mapping]
    F --> G[Presence ID]
    G --> H[presenceid_presence<br/>Presence Data]
    
    I[To-Device ID] --> J[todeviceid_events<br/>Device Messages]
    
    style D fill:#e8f5e8
    style H fill:#f3e5f5
    style J fill:#fff3e0
```

## 10. Global Configuration and Access Control

```mermaid
graph TD
    A[Global Config] --> B[global<br/>Server Settings]
    
    C[Room Categories] --> D[publicroomids<br/>Public Rooms]
    C --> E[bannedroomids<br/>Banned Rooms]
    C --> F[disabledroomids<br/>Disabled Rooms]
    
    G[App Service ID] --> H[id_appserviceregistrations<br/>Application Services]
    
    I[Token Management] --> J[tokenids<br/>Token Allocation]
    
    K[Relations] --> L[tofrom_relation<br/>Event Relations]
    K --> M[threadid_userids<br/>Thread Participants]
    K --> N[referencedevents<br/>Referenced Events]
    K --> O[softfailedeventids<br/>Failed Events]
    
    style B fill:#e1f5fe
    style D fill:#e8f5e8
    style E fill:#ffebee
    style F fill:#ffebee
```

## 11. Complete System Overview

```mermaid
graph TB
    subgraph "Identity Management"
        UI[User Identity]
        UA[User Auth]
        UD[User Devices]
        UP[User Profile]
    end
    
    subgraph "Event Storage"
        ES[Event Storage]
        EID[Event ID Mapping]
        EO[Outlier Events]
    end
    
    subgraph "Room Management"
        RS[Room State]
        RM[Room Membership]
        RMeta[Room Metadata]
    end
    
    subgraph "Cryptography"
        DK[Device Keys]
        CS[Cross Signing]
        KB[Key Backups]
    end
    
    subgraph "Federation"
        FS[Federation Servers]
        FK[Federation Keys]
        FE[Federation Events]
    end
    
    subgraph "Communication"
        PUSH[Push Notifications]
        RT[Read Tracking]
        DM[Device Messages]
    end
    
    subgraph "Content"
        MC[Media Content]
        UP2[URL Previews]
        AD[Account Data]
    end
    
    UI --> UA
    UA --> UD
    UI --> UP
    
    ES --> EID
    ES --> EO
    EID --> RS
    
    RS --> RM
    RM --> RMeta
    
    UD --> DK
    DK --> CS
    CS --> KB
    
    RM --> FS
    FS --> FK
    FK --> FE
    
    UD --> PUSH
    RM --> RT
    UD --> DM
    
    UI --> MC
    MC --> UP2
    UI --> AD
    
    style UI fill:#e8f5e8
    style ES fill:#e1f5fe
    style RS fill:#f3e5f5
    style DK fill:#fff3e0
    style FS fill:#f0f4ff
```

## Diagram Legend

- **Blue boxes** (`#e1f5fe`): Core storage tables
- **Green boxes** (`#e8f5e8`): Membership and relationship tables  
- **Purple boxes** (`#f3e5f5`): ID mapping and compression tables
- **Orange boxes** (`#fff3e0`): Count and metadata tables
- **Light blue boxes** (`#f0f4ff`): Sync and federation tables
- **Red boxes** (`#ffebee`): Access control and security tables
- **Solid arrows**: Direct relationships
- **Dotted arrows**: Bidirectional or related tables
- **Shared Cache notation**: Tables that share memory pools

These diagrams show how Continuwuity's 89 database tables interconnect to provide a complete Matrix homeserver implementation with optimized storage patterns and efficient relationship management.
