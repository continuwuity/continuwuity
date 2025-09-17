pub mod console;
mod create;
mod execute;
mod grant;

use std::{
	pin::Pin,
	sync::{Arc, Weak},
};

use async_trait::async_trait;
use conduwuit::{Err, SyncRwLock, utils};
use conduwuit_core::{
	Error, Event, Result, Server, debug, err, error, error::default_log, pdu::PduBuilder,
};
pub use create::create_admin_room;
use futures::{Future, FutureExt, TryFutureExt};
use loole::{Receiver, Sender};
use ruma::{
	Mxc, OwnedEventId, OwnedMxcUri, OwnedRoomId, RoomId, UInt, UserId,
	events::{
		Mentions,
		room::{
			MediaSource,
			message::{
				FileInfo, FileMessageEventContent, MessageType, Relation, RoomMessageEventContent,
			},
		},
	},
};
use tokio::sync::RwLock;

use crate::{Dep, account_data, globals, media::MXC_LENGTH, rooms, rooms::state::RoomMutexGuard};

pub struct Service {
	services: Services,
	channel: (Sender<CommandInput>, Receiver<CommandInput>),
	pub handle: RwLock<Option<Processor>>,
	pub complete: SyncRwLock<Option<Completer>>,
	#[cfg(feature = "console")]
	pub console: Arc<console::Console>,
}

struct Services {
	server: Arc<Server>,
	globals: Dep<globals::Service>,
	alias: Dep<rooms::alias::Service>,
	timeline: Dep<rooms::timeline::Service>,
	state: Dep<rooms::state::Service>,
	state_cache: Dep<rooms::state_cache::Service>,
	state_accessor: Dep<rooms::state_accessor::Service>,
	account_data: Dep<account_data::Service>,
	services: SyncRwLock<Option<Weak<crate::Services>>>,
	media: Dep<crate::media::Service>,
}

/// Inputs to a command are a multi-line string, optional reply_id, and optional
/// sender.
#[derive(Debug)]
pub struct CommandInput {
	pub command: String,
	pub reply_id: Option<OwnedEventId>,
	pub sender: Option<Box<UserId>>,
}

/// Prototype of the tab-completer. The input is buffered text when tab
/// asserted; the output will fully replace the input buffer.
pub type Completer = fn(&str) -> String;

/// Prototype of the command processor. This is a callback supplied by the
/// reloadable admin module.
pub type Processor = fn(Arc<crate::Services>, CommandInput) -> ProcessorFuture;

/// Return type of the processor
pub type ProcessorFuture = Pin<Box<dyn Future<Output = ProcessorResult> + Send>>;

/// Result wrapping of a command's handling. Both variants are complete message
/// events which have digested any prior errors. The wrapping preserves whether
/// the command failed without interpreting the text. Ok(None) outputs are
/// dropped to produce no response.
pub type ProcessorResult = Result<Option<CommandOutput>, CommandOutput>;

/// Alias for the output structure.
pub type CommandOutput = RoomMessageEventContent;

/// Maximum number of commands which can be queued for dispatch.
const COMMAND_QUEUE_LIMIT: usize = 512;

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				server: args.server.clone(),
				globals: args.depend::<globals::Service>("globals"),
				alias: args.depend::<rooms::alias::Service>("rooms::alias"),
				timeline: args.depend::<rooms::timeline::Service>("rooms::timeline"),
				state: args.depend::<rooms::state::Service>("rooms::state"),
				state_cache: args.depend::<rooms::state_cache::Service>("rooms::state_cache"),
				state_accessor: args
					.depend::<rooms::state_accessor::Service>("rooms::state_accessor"),
				account_data: args.depend::<account_data::Service>("account_data"),
				services: None.into(),
				media: args.depend::<crate::media::Service>("media"),
			},
			channel: loole::bounded(COMMAND_QUEUE_LIMIT),
			handle: RwLock::new(None),
			complete: SyncRwLock::new(None),
			#[cfg(feature = "console")]
			console: console::Console::new(&args),
		}))
	}

	async fn worker(self: Arc<Self>) -> Result<()> {
		let mut signals = self.services.server.signal.subscribe();
		let receiver = self.channel.1.clone();

		self.startup_execute().await?;
		self.console_auto_start().await;

		loop {
			tokio::select! {
				command = receiver.recv_async() => match command {
					Ok(command) => self.handle_command(command).await,
					Err(_) => break,
				},
				sig = signals.recv() => match sig {
					Ok(sig) => self.handle_signal(sig).await,
					Err(_) => continue,
				},
			}
		}

		self.console_auto_stop().await; //TODO: not unwind safe

		Ok(())
	}

	fn interrupt(&self) {
		#[cfg(feature = "console")]
		self.console.interrupt();

		let (sender, _) = &self.channel;
		if !sender.is_closed() {
			sender.close();
		}
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Sends markdown notice to the admin room as the admin user.
	pub async fn notice(&self, body: &str) {
		self.send_message(RoomMessageEventContent::notice_markdown(body))
			.await
			.ok();
	}

	/// Sends markdown message (not an m.notice for notification reasons) to the
	/// admin room as the admin user.
	pub async fn send_text(&self, body: &str) {
		self.send_message(RoomMessageEventContent::text_markdown(body))
			.await
			.ok();
	}

	/// Either returns a small-enough message, or converts a large message into
	/// a file
	pub async fn text_or_file(
		&self,
		message_content: RoomMessageEventContent,
	) -> RoomMessageEventContent {
		let body_len = Self::collate_msg_size(&message_content);
		if body_len > 60000 {
			// Intercept and send as file
			let file = self
				.text_to_file(message_content.body())
				.await
				.expect("failed to create text file");
			let size_u64: u64 = message_content.body().len().try_into().map_or(0, |n| n);
			let metadata = FileInfo {
				mimetype: Some("text/markdown".to_owned()),
				size: Some(UInt::new_saturating(size_u64)),
				thumbnail_info: None,
				thumbnail_source: None,
			};
			let content = FileMessageEventContent {
				body: "Output was too large to send as text.".to_owned(),
				formatted: None,
				filename: Some("output.md".to_owned()),
				source: MediaSource::Plain(file),
				info: Some(Box::new(metadata)),
			};
			RoomMessageEventContent::new(MessageType::File(content))
		} else {
			message_content
		}
	}

	#[must_use]
	pub fn collate_msg_size(content: &RoomMessageEventContent) -> u64 {
		content
			.body()
			.len()
			.saturating_add(match &content.msgtype {
				| MessageType::Text(t) =>
					if t.formatted.is_some() {
						t.formatted.as_ref().map_or(0, |f| f.body.len())
					} else {
						0
					},
				| MessageType::Notice(n) =>
					if n.formatted.is_some() {
						n.formatted.as_ref().map_or(0, |f| f.body.len())
					} else {
						0
					},
				| _ => 0,
			})
			.try_into()
			.expect("size too large")
	}

	/// Sends a message to the admin room as the admin user (see send_text()
	/// for convenience).
	pub async fn send_message(&self, message_content: RoomMessageEventContent) -> Result<()> {
		let user_id = &self.services.globals.server_user;
		let room_id = self.get_admin_room().await?;
		self.respond_to_room(message_content, &room_id, user_id)
			.boxed()
			.await
	}

	/// Sends a message, the same as send_message() but with an @room ping to
	/// notify all users in the room.
	pub async fn send_loud_message(
		&self,
		mut message_content: RoomMessageEventContent,
	) -> Result<()> {
		// Add @room ping
		message_content = message_content.add_mentions(Mentions::with_room_mention());
		self.send_message(message_content).await
	}

	/// Casts a text body into a file and creates a file for it.
	pub async fn text_to_file(&self, body: &str) -> Result<OwnedMxcUri> {
		let mxc = Mxc {
			server_name: self.services.globals.server_name(),
			media_id: &utils::random_string(MXC_LENGTH),
		};
		match self
			.services
			.media
			.create(
				&mxc,
				Some(self.services.globals.server_user.as_ref()),
				Some(&utils::content_disposition::make_content_disposition(
					None,
					Some("text/markdown"),
					Some("output.md"),
				)),
				Some("text/markdown"),
				body.as_bytes(),
			)
			.await
		{
			| Ok(()) => Ok(mxc.to_string().into()),
			| Err(e) => {
				error!("Failed to upload text to file: {e}");
				Err!(Request(Unknown("Failed to upload text to file")))
			},
		}
	}

	/// Posts a command to the command processor queue and returns. Processing
	/// will take place on the service worker's task asynchronously. Errors if
	/// the queue is full.
	pub fn command(&self, command: String, reply_id: Option<OwnedEventId>) -> Result<()> {
		self.channel
			.0
			.send(CommandInput { command, reply_id, sender: None })
			.map_err(|e| err!("Failed to enqueue admin command: {e:?}"))
	}

	/// Posts a command to the command processor queue with sender information
	/// and returns. Processing will take place on the service worker's task
	/// asynchronously. Errors if the queue is full.
	pub fn command_with_sender(
		&self,
		command: String,
		reply_id: Option<OwnedEventId>,
		sender: Box<UserId>,
	) -> Result<()> {
		self.channel
			.0
			.send(CommandInput { command, reply_id, sender: Some(sender) })
			.map_err(|e| err!("Failed to enqueue admin command: {e:?}"))
	}

	/// Dispatches a command to the processor on the current task and waits for
	/// completion.
	pub async fn command_in_place(
		&self,
		command: String,
		reply_id: Option<OwnedEventId>,
	) -> ProcessorResult {
		self.process_command(CommandInput { command, reply_id, sender: None })
			.await
	}

	/// Invokes the tab-completer to complete the command. When unavailable,
	/// None is returned.
	pub fn complete_command(&self, command: &str) -> Option<String> {
		self.complete.read().map(|complete| complete(command))
	}

	async fn handle_signal(&self, sig: &'static str) {
		if sig == execute::SIGNAL {
			self.signal_execute().await.ok();
		}

		#[cfg(feature = "console")]
		self.console.handle_signal(sig).await;
	}

	async fn handle_command(&self, command: CommandInput) {
		match self.process_command(command).await {
			| Ok(None) => debug!("Command successful with no response"),
			| Ok(Some(output)) | Err(output) => self
				.handle_response(output)
				.await
				.unwrap_or_else(default_log),
		}
	}

	async fn process_command(&self, command: CommandInput) -> ProcessorResult {
		let handle_guard = self.handle.read().await;
		let handle = handle_guard.as_ref().expect("Admin module is not loaded");

		let services = self
			.services
			.services
			.read()
			.as_ref()
			.and_then(Weak::upgrade)
			.expect("Services self-reference not initialized.");

		handle(services, command).await
	}

	/// Checks whether a given user is an admin of this server
	pub async fn user_is_admin(&self, user_id: &UserId) -> bool {
		let Ok(admin_room) = self.get_admin_room().await else {
			return false;
		};

		self.services
			.state_cache
			.is_joined(user_id, &admin_room)
			.await
	}

	/// Gets the room ID of the admin room
	///
	/// Errors are propagated from the database, and will have None if there is
	/// no admin room
	pub async fn get_admin_room(&self) -> Result<OwnedRoomId> {
		let room_id = self
			.services
			.alias
			.resolve_local_alias(&self.services.globals.admin_alias)
			.await?;

		self.services
			.state_cache
			.is_joined(&self.services.globals.server_user, &room_id)
			.await
			.then_some(room_id)
			.ok_or_else(|| err!(Request(NotFound("Admin user not joined to admin room"))))
	}

	async fn handle_response(&self, content: RoomMessageEventContent) -> Result<()> {
		let Some(Relation::Reply { in_reply_to }) = content.relates_to.as_ref() else {
			return Ok(());
		};

		let Ok(pdu) = self.services.timeline.get_pdu(&in_reply_to.event_id).await else {
			error!(
				event_id = ?in_reply_to.event_id,
				"Missing admin command in_reply_to event"
			);
			return Ok(());
		};

		let response_sender = if self.is_admin_room(pdu.room_id().unwrap()).await {
			&self.services.globals.server_user
		} else {
			pdu.sender()
		};

		self.respond_to_room(content, pdu.room_id().unwrap(), response_sender)
			.boxed()
			.await
	}

	async fn respond_to_room(
		&self,
		content: RoomMessageEventContent,
		room_id: &RoomId,
		user_id: &UserId,
	) -> Result<()> {
		assert!(self.user_is_admin(user_id).await, "sender is not admin");

		let state_lock = self.services.state.mutex.lock(room_id).await;
		if let Err(e) = self
			.services
			.timeline
			.build_and_append_pdu(
				PduBuilder::timeline(&self.text_or_file(content).await),
				user_id,
				Some(room_id),
				&state_lock,
			)
			.await
		{
			self.handle_response_error(e, room_id, user_id, &state_lock)
				.boxed()
				.await
				.unwrap_or_else(default_log);
		}

		Ok(())
	}

	async fn handle_response_error(
		&self,
		e: Error,
		room_id: &RoomId,
		user_id: &UserId,
		state_lock: &RoomMutexGuard,
	) -> Result<()> {
		error!("Failed to build and append admin room response PDU: \"{e}\"");
		let content = RoomMessageEventContent::text_plain(format!(
			"Failed to build and append admin room PDU: \"{e}\"\n\nThe original admin command \
			 may have finished successfully, but we could not return the output."
		));

		self.services
			.timeline
			.build_and_append_pdu(
				PduBuilder::timeline(&content),
				user_id,
				Some(room_id),
				state_lock,
			)
			.await?;

		Ok(())
	}

	pub async fn is_admin_command<E>(&self, event: &E, body: &str) -> bool
	where
		E: Event + Send + Sync,
	{
		// Server-side command-escape with public echo
		let is_escape = body.starts_with('\\');
		let is_public_escape = is_escape && body.trim_start_matches('\\').starts_with("!admin");

		// Admin command with public echo (in admin room)
		let server_user = &self.services.globals.server_user;
		let is_public_prefix =
			body.starts_with("!admin") || body.starts_with(server_user.as_str());

		// Expected backward branch
		if !is_public_escape && !is_public_prefix {
			return false;
		}

		let user_is_local = self.services.globals.user_is_local(event.sender());

		// only allow public escaped commands by local admins
		if is_public_escape && !user_is_local {
			return false;
		}

		// Check if server-side command-escape is disabled by configuration
		if is_public_escape && !self.services.server.config.admin_escape_commands {
			return false;
		}

		// Prevent unescaped !admin from being used outside of the admin room
		if event.room_id().is_some()
			&& is_public_prefix
			&& !self.is_admin_room(event.room_id().unwrap()).await
		{
			return false;
		}

		// Only senders who are admin can proceed
		if !self.user_is_admin(event.sender()).await {
			return false;
		}

		// This will evaluate to false if the emergency password is set up so that
		// the administrator can execute commands as the server user
		let emergency_password_set = self.services.server.config.emergency_password.is_some();
		let from_server = event.sender() == server_user && !emergency_password_set;
		if from_server && self.is_admin_room(event.room_id().unwrap()).await {
			return false;
		}

		// Authentic admin command
		true
	}

	#[must_use]
	pub async fn is_admin_room(&self, room_id_: &RoomId) -> bool {
		self.get_admin_room()
			.map_ok(|room_id| room_id == room_id_)
			.await
			.unwrap_or(false)
	}

	/// Sets the self-reference to crate::Services which will provide context to
	/// the admin commands.
	pub(super) fn set_services(&self, services: Option<&Arc<crate::Services>>) {
		let receiver = &mut *self.services.services.write();
		let weak = services.map(Arc::downgrade);
		*receiver = weak;
	}
}
