mod commands;

use clap::Subcommand;
use conduwuit::Result;
use ruma::{OwnedEventId, OwnedRoomId, OwnedRoomOrAliasId};

use crate::admin_command_dispatch;

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub enum UserCommand {
	/// - Create a new user
	#[clap(alias = "create")]
	CreateUser {
		/// Username of the new user
		username: String,
		/// Password of the new user, if unspecified one is generated
		password: Option<String>,
	},

	/// - Reset user password
	ResetPassword {
		/// Username of the user for whom the password should be reset
		username: String,
		/// New password for the user, if unspecified one is generated
		password: Option<String>,
	},

	/// - Deactivate a user
	///
	/// User will be removed from all rooms by default.
	/// Use --no-leave-rooms to not leave all rooms by default.
	Deactivate {
		#[arg(short, long)]
		no_leave_rooms: bool,
		user_id: String,
	},

	/// - Deactivate a list of users
	///
	/// Recommended to use in conjunction with list-local-users.
	///
	/// Users will be removed from joined rooms by default.
	///
	/// Can be overridden with --no-leave-rooms.
	///
	/// Removing a mass amount of users from a room may cause a significant
	/// amount of leave events. The time to leave rooms may depend significantly
	/// on joined rooms and servers.
	///
	/// This command needs a newline separated list of users provided in a
	/// Markdown code block below the command.
	DeactivateAll {
		#[arg(short, long)]
		/// Does not leave any rooms the user is in on deactivation
		no_leave_rooms: bool,
		#[arg(short, long)]
		/// Also deactivate admin accounts and will assume leave all rooms too
		force: bool,
	},

	/// - Forcefully log a user out of all of their devices.
	///
	/// This will invalidate all access tokens for the specified user,
	/// effectively logging them out from all sessions.
	/// Note that this is destructive and may result in data loss for the user,
	/// such as encryption keys. Use with caution. Can only be used in the admin
	/// room.
	Logout {
		/// Username of the user to log out
		user_id: String,
	},

	/// - Suspend a user
	///
	/// Suspended users are able to log in, sync, and read messages, but are not
	/// able to send events nor redact them, cannot change their profile, and
	/// are unable to join, invite to, or knock on rooms.
	///
	/// Suspended users can still leave rooms and deactivate their account.
	/// Suspending them effectively makes them read-only.
	Suspend {
		/// Username of the user to suspend
		user_id: String,
	},

	/// - Unsuspend a user
	///
	/// Reverses the effects of the `suspend` command, allowing the user to send
	/// messages, change their profile, create room invites, etc.
	Unsuspend {
		/// Username of the user to unsuspend
		user_id: String,
	},

	/// - Lock a user
	///
	/// Locked users are unable to use their accounts beyond logging out. This
	/// is akin to a temporary deactivation that does not change the user's
	/// password. This can be used to quickly prevent a user from accessing
	/// their account.
	Lock {
		/// Username of the user to lock
		user_id: String,
	},

	/// - Unlock a user
	///
	/// Reverses the effects of the `lock` command, allowing the user to use
	/// their account again.
	Unlock {
		/// Username of the user to unlock
		user_id: String,
	},

	/// - List local users in the database
	#[clap(alias = "list")]
	ListUsers,

	/// - Lists all the rooms (local and remote) that the specified user is
	///   joined in
	ListJoinedRooms {
		user_id: String,
	},

	/// - Manually join a local user to a room.
	ForceJoinRoom {
		user_id: String,
		room_id: OwnedRoomOrAliasId,
	},

	/// - Manually leave a local user from a room.
	ForceLeaveRoom {
		user_id: String,
		room_id: OwnedRoomOrAliasId,
	},

	/// - Manually leave a remote room for a local user.
	ForceLeaveRemoteRoom {
		user_id: String,
		room_id: OwnedRoomOrAliasId,
		via: Option<String>,
	},

	/// - Forces the specified user to drop their power levels to the room
	///   default, if their permissions allow and the auth check permits
	ForceDemote {
		user_id: String,
		room_id: OwnedRoomOrAliasId,
	},

	/// - Grant server-admin privileges to a user.
	MakeUserAdmin {
		user_id: String,
	},

	/// - Puts a room tag for the specified user and room ID.
	///
	/// This is primarily useful if you'd like to set your admin room
	/// to the special "System Alerts" section in Element as a way to
	/// permanently see your admin room without it being buried away in your
	/// favourites or rooms. To do this, you would pass your user, your admin
	/// room's internal ID, and the tag name `m.server_notice`.
	PutRoomTag {
		user_id: String,
		room_id: OwnedRoomId,
		tag: String,
	},

	/// - Deletes the room tag for the specified user and room ID
	DeleteRoomTag {
		user_id: String,
		room_id: OwnedRoomId,
		tag: String,
	},

	/// - Gets all the room tags for the specified user and room ID
	GetRoomTags {
		user_id: String,
		room_id: OwnedRoomId,
	},

	/// - Attempts to forcefully redact the specified event ID from the sender
	///   user
	///
	/// This is only valid for local users
	RedactEvent {
		event_id: OwnedEventId,
	},

	/// - Force joins a specified list of local users to join the specified
	///   room.
	///
	/// Specify a codeblock of usernames.
	///
	/// At least 1 server admin must be in the room to reduce abuse.
	///
	/// Requires the `--yes-i-want-to-do-this` flag.
	ForceJoinListOfLocalUsers {
		room_id: OwnedRoomOrAliasId,

		#[arg(long)]
		yes_i_want_to_do_this: bool,
	},

	/// - Force joins all local users to the specified room.
	///
	/// At least 1 server admin must be in the room to reduce abuse.
	///
	/// Requires the `--yes-i-want-to-do-this` flag.
	ForceJoinAllLocalUsers {
		room_id: OwnedRoomOrAliasId,

		#[arg(long)]
		yes_i_want_to_do_this: bool,
	},
}
