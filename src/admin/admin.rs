use clap::Parser;
use conduwuit::{Err, Result};

use crate::{
	appservice::{self, AppserviceCommand},
	check::{self, CheckCommand},
	context::Context,
	debug::{self, DebugCommand},
	federation::{self, FederationCommand},
	media::{self, MediaCommand},
	oidc::{self, OidcCommand},
	query::{self, QueryCommand},
	room::{self, RoomCommand},
	server::{self, ServerCommand},
	token::{self, TokenCommand},
	user::{self, UserCommand},
};

#[derive(Debug, Parser)]
#[command(name = conduwuit_core::BRANDING, version = conduwuit_core::version())]
pub enum AdminCommand {
	/// Commands for managing appservices
	#[command(subcommand)]
	Appservices(AppserviceCommand),

	/// Commands for managing local users
	#[command(subcommand)]
	Users(UserCommand),

	/// Commands for managing registration tokens
	#[command(subcommand)]
	Token(TokenCommand),

	/// Commands for managing OIDC
	#[command(subcommand)]
	Oidc(OidcCommand),

	/// Commands for managing rooms
	#[command(subcommand)]
	Rooms(RoomCommand),

	/// Commands for managing federation
	#[command(subcommand)]
	Federation(FederationCommand),

	/// Commands for managing the server
	#[command(subcommand)]
	Server(ServerCommand),

	/// Commands for managing media
	#[command(subcommand)]
	Media(MediaCommand),

	/// Commands for checking integrity
	#[command(subcommand)]
	Check(CheckCommand),

	/// Commands for debugging things
	#[command(subcommand)]
	Debug(DebugCommand),

	/// Low-level queries for database getters and iterators
	#[command(subcommand)]
	Query(QueryCommand),
}

#[tracing::instrument(skip_all, name = "command", level = "info")]
pub(super) async fn process(command: AdminCommand, context: &Context<'_>) -> Result {
	use AdminCommand::*;

	match command {
		| Appservices(command) => {
			// appservice commands are all restricted
			context.bail_restricted()?;
			appservice::process(command, context).await
		},
		| Media(command) => media::process(command, context).await,
		| Users(command) => {
			// user commands are all restricted
			context.bail_restricted()?;
			user::process(command, context).await
		},
		| Token(command) => {
			// token commands are all restricted
			context.bail_restricted()?;
			token::process(command, context).await
		},
		| Oidc(command) => {
			// OIDC commands are all restricted
			context.bail_restricted()?;

			if !context.services.oidc.enabled() {
				return Err!("OIDC is not configured");
			}

			oidc::process(command, context).await
		},
		| Rooms(command) => room::process(command, context).await,
		| Federation(command) => federation::process(command, context).await,
		| Server(command) => server::process(command, context).await,
		| Debug(command) => debug::process(command, context).await,
		| Query(command) => {
			// query commands are all restricted
			context.bail_restricted()?;
			query::process(command, context).await
		},
		| Check(command) => check::process(command, context).await,
	}
}
