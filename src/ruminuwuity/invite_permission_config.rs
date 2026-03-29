//! Types for invite filtering ([MSC4155]).
//! 
//! MSC4155: https://github.com/matrix-org/matrix-spec-proposals/pull/4155

use ruma::{ServerName, UserId};
use ruma::exports::ruma_macros::EventContent;
use serde::{Deserialize, Serialize};
use wildmatch::WildMatch;

/// Represents a user's level of filtering on actions from another user or server.
/// "Ignore" and "block" are defined in [MSC4283].
///
/// MSC4283: https://github.com/matrix-org/matrix-spec-proposals/pull/4283
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterLevel {
    Allow,
    Ignore,
    Block,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, EventContent)]
#[cfg_attr(not(feature = "unstable-exhaustive-types"), non_exhaustive)]
#[ruma_event(type = "m.invite_permission_config", kind = GlobalAccountData)]
pub struct InvitePermissionConfigEventContent {
    /// A global on/off toggle for all rules
    #[serde(default = "ruma::serde::default_true")]
    pub enabled: bool,
    
    /// A list of globs matching users which are allowed to send an invite.
    /// Entries in this list supersede entries in the ignored and blocked lists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_users: Vec<String>,
    /// A list of globs matching users whose invites should be ignored (as defined in [MSC4283]).
    /// 
    /// MSC4283: https://github.com/matrix-org/matrix-spec-proposals/pull/4283
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignored_users: Vec<String>,
    /// A list of globs matching users whose invites should be blocked (as defined in [MSC4283]).
    /// Invites from blocked users should be refused with the M_INVITE_BLOCKED status code.
    /// 
    /// MSC4283: https://github.com/matrix-org/matrix-spec-proposals/pull/4283
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_users: Vec<String>,


    /// A list of globs matching servers which are allowed to send an invite.
    /// Entries in this list supersede entries in the ignored and blocked lists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_servers: Vec<String>,
    /// A list of globs matching servers whose invites should be ignored (as defined in [MSC4283]).
    /// 
    /// MSC4283: https://github.com/matrix-org/matrix-spec-proposals/pull/4283
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignored_servers: Vec<String>,
    /// A list of globs matching servers whose invites should be blocked (as defined in [MSC4283]).
    /// Invites from blocked servers should be refused with the M_INVITE_BLOCKED status code.
    /// 
    /// MSC4283: https://github.com/matrix-org/matrix-spec-proposals/pull/4283
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_servers: Vec<String>,
}

impl InvitePermissionConfigEventContent {
    /// Creates a new `InvitePermissionConfigEventContent` from six lists of globs.
    #[must_use]
    pub fn new(
        enabled: bool,
        allowed_users: Vec<String>,
        ignored_users: Vec<String>,
        blocked_users: Vec<String>,
        allowed_servers: Vec<String>,
        ignored_servers: Vec<String>,
        blocked_servers: Vec<String>,
    ) -> Self {
        Self {
            enabled,
            allowed_users,
            ignored_users,
            blocked_users,
            allowed_servers,
            ignored_servers,
            blocked_servers,
        }
    }

    /// Test the filters against a user id. This function will check both the
    /// user rules _and_ the server rules.
    #[must_use]
    #[allow(clippy::if_same_then_else)]
    pub fn user_filter_level(&self, user: &UserId) -> FilterLevel {
        if !self.enabled {
            FilterLevel::Allow
        } else if Self::matches(&self.allowed_users, user.as_str()) {
            FilterLevel::Allow
        } else if Self::matches(&self.ignored_users, user.as_str()) {
            FilterLevel::Ignore
        } else if Self::matches(&self.blocked_users, user.as_str()) {
            FilterLevel::Block
        } else {
            self.server_filter_level(user.server_name())
        }
    }

    /// Test the filters against a server name. Port numbers are ignored.
    #[must_use]
    pub fn server_filter_level(&self, server: &ServerName) -> FilterLevel {
        if !self.enabled {
            FilterLevel::Allow
        } else {
            let server = server.host();
            if Self::matches(&self.allowed_servers, server) {
                FilterLevel::Allow
            } else if Self::matches(&self.ignored_servers, server) {
                FilterLevel::Ignore
            } else if Self::matches(&self.blocked_servers, server) {
                FilterLevel::Block
            } else {
                FilterLevel::Allow
            }
        }
    }

    fn matches(a: &[String], s: &str) -> bool {
        a.iter().map(String::as_str).any(|a| WildMatch::new(a).matches(s))
    }
}

#[cfg(test)]
mod tests {
    use ruma::{ServerName, UserId, events::GlobalAccountDataEvent};
    use serde_json::{from_value as from_json_value, json};

    use crate::invite_permission_config::{FilterLevel, InvitePermissionConfigEventContent};

    fn user_id(id: &str) -> &UserId {
        <&UserId>::try_from(id).unwrap()
    }

    fn server_name(name: &str) -> &ServerName {
        <&ServerName>::try_from(name).unwrap()
    }

    #[test]
    fn default_values() {
        let data = json!({
            "content": {},
            "type": "org.matrix.msc4155.invite_permission_config"
        });

        let event: GlobalAccountDataEvent<InvitePermissionConfigEventContent> = from_json_value(data).unwrap();
        assert!(event.content.enabled);
        assert!(event.content.allowed_users.is_empty());
        assert!(event.content.ignored_users.is_empty());
        assert!(event.content.blocked_users.is_empty());
        assert!(event.content.allowed_servers.is_empty());
        assert!(event.content.ignored_servers.is_empty());
        assert!(event.content.blocked_servers.is_empty());
        assert_eq!(event.content.user_filter_level(user_id("@alice:example.com")), FilterLevel::Allow);
        assert_eq!(event.content.server_filter_level(server_name("example.com")), FilterLevel::Allow);
    }

    #[test]
    fn block_the_world() {
        let event = InvitePermissionConfigEventContent {
            enabled: true,
            blocked_servers: vec!["*".to_owned()],
            ..Default::default()
        };

        assert_eq!(event.user_filter_level(user_id("@alice:foo.com:8080")), FilterLevel::Block);
        assert_eq!(event.user_filter_level(user_id("@bob:bar.com")), FilterLevel::Block);
    }

    #[test]
    fn only_goodguys() {
        let event = InvitePermissionConfigEventContent {
            enabled: true,
            allowed_servers: vec!["goodguys.org".to_owned()],
            blocked_servers: vec!["*".to_owned()],
            ..Default::default()
        };

        assert_eq!(event.user_filter_level(user_id("@alice:goodguys.org:8080")), FilterLevel::Allow);
        assert_eq!(event.user_filter_level(user_id("@alice:goodguys.org")), FilterLevel::Allow);
        assert_eq!(event.user_filter_level(user_id("@bob:bar.com")), FilterLevel::Block);
    }

    #[test]
    fn exclude_badguys() {
        let event = InvitePermissionConfigEventContent {
            enabled: true,
            blocked_servers: vec!["badguys.org".to_owned()],
            ..Default::default()
        };

        assert_eq!(event.user_filter_level(user_id("@alice:goodguys.org")), FilterLevel::Allow);
        assert_eq!(event.user_filter_level(user_id("@bob:bar.com")), FilterLevel::Allow);
        assert_eq!(event.user_filter_level(user_id("@kevin:badguys.org:8080")), FilterLevel::Block);
        assert_eq!(event.user_filter_level(user_id("@kevin:badguys.org")), FilterLevel::Block);
    }

    #[test]
    fn only_goodguys_except_for_kevin() {
        let event = InvitePermissionConfigEventContent {
            enabled: true,
            blocked_users: vec!["@kevin:goodguys.org".to_owned()],
            allowed_servers: vec!["goodguys.org".to_owned()],
            blocked_servers: vec!["*".to_owned()],
            ..Default::default()
        };

        assert_eq!(event.user_filter_level(user_id("@alice:goodguys.org")), FilterLevel::Allow);
        assert_eq!(event.user_filter_level(user_id("@kevin:goodguys.org")), FilterLevel::Block);
        assert_eq!(event.user_filter_level(user_id("@kevin:badguys.org")), FilterLevel::Block);
    }

    #[test]
    fn no_badguys_except_for_alice() {
        let event = InvitePermissionConfigEventContent {
            enabled: true,
            allowed_users: vec!["@alice:badguys.org".to_owned()],
            blocked_servers: vec!["badguys.org".to_owned()],
            ..Default::default()
        };

        assert_eq!(event.user_filter_level(user_id("@alice:goodguys.org")), FilterLevel::Allow);
        assert_eq!(event.user_filter_level(user_id("@alice:badguys.org")), FilterLevel::Allow);
        assert_eq!(event.user_filter_level(user_id("@bob:bar.com")), FilterLevel::Allow);
        assert_eq!(event.user_filter_level(user_id("@kevin:badguys.org")), FilterLevel::Block);
    }

    #[test]
    fn only_goodguys_and_ignore_reallybadguys() {
        let event = InvitePermissionConfigEventContent {
            enabled: true,
            allowed_servers: vec!["goodguys.org".to_owned()],
            ignored_servers: vec!["reallybadguys.org".to_owned()],
            blocked_servers: vec!["*".to_owned()],
            ..Default::default()
        };

        assert_eq!(event.user_filter_level(user_id("@alice:goodguys.org:8080")), FilterLevel::Allow);
        assert_eq!(event.user_filter_level(user_id("@alice:goodguys.org")), FilterLevel::Allow);
        assert_eq!(event.user_filter_level(user_id("@bob:bar.com")), FilterLevel::Block);
        assert_eq!(event.user_filter_level(user_id("@kevin:reallybadguys.org")), FilterLevel::Ignore);
    }
}