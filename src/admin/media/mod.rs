#![allow(rustdoc::broken_intra_doc_links)]
mod commands;

use clap::Subcommand;
use conduwuit::Result;
use ruma::{OwnedEventId, OwnedMxcUri, OwnedServerName};

use crate::admin_command_dispatch;

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub enum MediaCommand {
	/// Deletes a single media file from our database and on the filesystem
	///   via a single MXC URL or event ID (not redacted)
	Delete {
		/// The MXC URL to delete
		#[arg(long)]
		mxc: Option<OwnedMxcUri>,

		/// The message event ID which contains the media and thumbnail MXC
		///   URLs
		#[arg(long)]
		event_id: Option<OwnedEventId>,
	},

	/// Deletes a codeblock list of MXC URLs from our database and on the
	///   filesystem. This will always ignore errors.
	DeleteList,

	/// Deletes all remote (and optionally local) media created before/after
	/// [duration] ago, using filesystem metadata first created at date, or
	/// fallback to last modified date. This will always ignore errors by
	/// default.
	///
	/// * Examples:
	///   * Delete all remote media older than a year:
	///
	///     `!admin media delete-past-remote-media -b 1y`
	///
	///   * Delete all remote and local media from 3 days ago, up until now:
	///
	///     `!admin media delete-past-remote-media -a 3d
	///-yes-i-want-to-delete-local-media`
	#[command(verbatim_doc_comment)]
	DeletePastRemoteMedia {
		/// The relative time (e.g. 30s, 5m, 7d) from now within which to
		///   search
		duration: String,

		/// Only delete media created before [duration] ago
		#[arg(long, short)]
		before: bool,

		/// Only delete media created after [duration] ago
		#[arg(long, short)]
		after: bool,

		/// Long argument to additionally delete local media
		#[arg(long)]
		yes_i_want_to_delete_local_media: bool,
	},

	/// Deletes all the local media from a local user on our server. This will
	///   always ignore errors by default.
	DeleteAllFromUser {
		username: String,
	},

	/// Deletes all remote media from the specified remote server. This will
	///   always ignore errors by default.
	DeleteAllFromServer {
		server_name: OwnedServerName,

		/// Long argument to delete local media
		#[arg(long)]
		yes_i_want_to_delete_local_media: bool,
	},

	GetFileInfo {
		/// The MXC URL to lookup info for.
		mxc: OwnedMxcUri,
	},

	GetRemoteFile {
		/// The MXC URL to fetch
		mxc: OwnedMxcUri,

		#[arg(short, long)]
		server: Option<OwnedServerName>,

		#[arg(short, long, default_value("10000"))]
		timeout: u32,
	},

	GetRemoteThumbnail {
		/// The MXC URL to fetch
		mxc: OwnedMxcUri,

		#[arg(short, long)]
		server: Option<OwnedServerName>,

		#[arg(short, long, default_value("10000"))]
		timeout: u32,

		#[arg(long, default_value("800"))]
		width: u32,

		#[arg(long, default_value("800"))]
		height: u32,
	},

	/// Deletes a cached URL preview, forcing it to be re-fetched.
	/// Use --all to purge all cached URL previews.
	DeleteUrlPreview {
		/// The URL to clear from the saved preview data
		#[arg(required_unless_present = "all")]
		url: Option<String>,

		/// Purge all cached URL previews
		#[arg(long, conflicts_with = "url")]
		all: bool,
	},
}
