use std::{
	io::IsTerminal,
	sync::{Arc, OnceLock},
};

use askama::Template;
use async_trait::async_trait;
use conduwuit::{Result, info, utils::ReadyExt};
use futures::StreamExt;
use ruma::{UserId, events::room::message::RoomMessageEventContent};

use crate::{
	Dep, admin, config, globals,
	registration_tokens::{self, ValidToken, ValidTokenSource},
	users,
};

pub struct Service {
	services: Services,
	/// Represents the state of first run mode.
	///
	/// First run mode is either active or inactive at server start. It may
	/// transition from active to inactive, but only once, and can never
	/// transition the other way. Additionally, whether the server is in first
	/// run mode or not can only be determined when all services are
	/// constructed. The outer `OnceLock` represents the unknown state of first
	/// run mode, and the inner `OnceLock` enforces the one-time transition from
	/// active to inactive.
	///
	/// Consequently, this marker may be in one of three states:
	/// 1. OnceLock<uninitialized>, representing the unknown state of first run
	///    mode during server startup. Once server startup is complete, the
	///    marker transitions to state 2 or directly to state 3.
	/// 2. OnceLock<OnceLock<uninitialized>>, representing first run mode being
	///    active. The marker may only transition to state 3 from here.
	/// 3. OnceLock<OnceLock<()>>, representing first run mode being inactive.
	///    The marker may not transition out of this state.
	first_run_marker: OnceLock<OnceLock<()>>,
	/// A single-use registration token which may be used to create the first
	/// account.
	first_account_token: String,
}

struct Services {
	config: Dep<config::Service>,
	users: Dep<users::Service>,
	globals: Dep<globals::Service>,
	admin: Dep<admin::Service>,
}

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				config: args.depend::<config::Service>("config"),
				users: args.depend::<users::Service>("users"),
				globals: args.depend::<globals::Service>("globals"),
				admin: args.depend::<admin::Service>("admin"),
			},
			// marker starts in an indeterminate state
			first_run_marker: OnceLock::new(),
			first_account_token: registration_tokens::Service::generate_token_string(),
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }

	async fn worker(self: Arc<Self>) -> Result {
		// first run mode will be enabled if there are no local users, provided it's not
		// forcibly disabled for Complement tests
		let is_first_run = !self.services.config.force_disable_first_run_mode
			&& self
				.services
				.users
				.list_local_users()
				.ready_filter(|user| *user != self.services.globals.server_user)
				.next()
				.await
				.is_none();

		self.first_run_marker
			.set(if is_first_run {
				// first run mode is active (empty inner lock)
				OnceLock::new()
			} else {
				// first run mode is inactive (already filled inner lock)
				OnceLock::from(())
			})
			.expect("Service worker should only be called once");

		Ok(())
	}
}

impl Service {
	/// Check if first run mode is active.
	pub fn is_first_run(&self) -> bool {
		self.first_run_marker
			.get()
			.expect("First run mode should not be checked during server startup")
			.get()
			.is_none()
	}

	/// Disable first run mode and begin normal operation.
	///
	/// Returns true if first run mode was successfully disabled, and false if
	/// first run mode was already disabled.
	fn disable_first_run(&self) -> bool {
		self.first_run_marker
			.get()
			.expect("First run mode should not be disabled during server startup")
			.set(())
			.is_ok()
	}

	/// If first-run mode is active, grant admin powers to the specified user
	/// and disable first-run mode.
	///
	/// Returns Ok(true) if the specified user was the first user, and Ok(false)
	/// if they were not.
	pub async fn empower_first_user(&self, user: &UserId) -> Result<bool> {
		#[derive(Template)]
		#[template(path = "welcome.md.j2")]
		struct WelcomeMessage<'a> {
			config: &'a Dep<config::Service>,
			domain: &'a str,
		}

		// If first run mode isn't active, do nothing.
		if !self.disable_first_run() {
			return Ok(false);
		}

		self.services.admin.make_user_admin(user).await?;

		// Send the welcome message
		let welcome_message = WelcomeMessage {
			config: &self.services.config,
			domain: self.services.globals.server_name().as_str(),
		}
		.render()
		.expect("should have been able to render welcome message template");

		self.services
			.admin
			.send_loud_message(RoomMessageEventContent::text_markdown(welcome_message))
			.await?;

		info!("{user} has been invited to the admin room as the first user.");

		Ok(true)
	}

	/// Get the single-use registration token which may be used to create the
	/// first account.
	pub fn get_first_account_token(&self) -> Option<ValidToken> {
		if self.is_first_run() {
			Some(ValidToken {
				token: self.first_account_token.clone(),
				source: ValidTokenSource::FirstAccount,
			})
		} else {
			None
		}
	}

	pub fn print_first_run_banner(&self) {
		use yansi::Paint;
		// This function is specially called by the core after all other
		// services have started. It runs last to ensure that the banner it
		// prints comes after any other logging which may occur on startup.

		if !self.is_first_run() {
			return;
		}

		eprintln!();
		eprintln!("{}", "============".bold());
		eprintln!(
			"Welcome to {} {}!",
			"Continuwuity".bold().bright_magenta(),
			conduwuit::version::version().bold()
		);
		eprintln!();
		eprintln!(
			"In order to use your new homeserver, you need to create its first user account."
		);
		eprintln!(
			"Open your Matrix client of choice and register an account on {} using the \
			 registration token {} . Pick your own username and password!",
			self.services.globals.server_name().bold().green(),
			self.first_account_token.as_str().bold().green()
		);

		match (
			self.services.config.allow_registration,
			self.services.config.get_config_file_token().is_some(),
		) {
			| (true, true) => {
				eprintln!(
					"{} until you create an account using the token above.",
					"The registration token you set in your configuration will not function"
						.red()
				);
			},
			| (true, false) => {
				eprintln!(
					"{} until you create an account using the token above.",
					"Nobody else will be able to register".green()
				);
			},
			| (false, true) => {
				eprintln!(
					"{} because you have disabled registration in your configuration. If this \
					 is not desired, set `allow_registration` to true and restart Continuwuity.",
					"The registration token you set in your configuration will not be usable"
						.yellow()
				);
			},
			| (false, false) => {
				eprintln!(
					"{} to allow you to create an account. Because registration is not enabled \
					 in your configuration, it will be disabled again once your account is \
					 created.",
					"Registration has been temporarily enabled".yellow()
				);
			},
		}
		eprintln!(
			"{} https://matrix.org/ecosystem/clients/",
			"Find a list of Matrix clients here:".bold()
		);

		if self.services.config.suspend_on_register {
			eprintln!(
				"{} Because you enabled suspend-on-register in your configuration, accounts \
				 created after yours will be automatically suspended.",
				"Your account will not be suspended when you register.".green()
			);
		}

		if self
			.services
			.config
			.yes_i_am_very_very_sure_i_want_an_open_registration_server_prone_to_abuse
		{
			eprintln!();
			eprintln!(
				"{}",
				"You have enabled open registration in your configuration! You almost certainly \
				 do not want to do this."
					.bold()
					.on_red()
			);
			eprintln!(
				"{}",
				"Servers with open, unrestricted registration are prone to abuse by spammers. \
				 Users on your server may be unable to join chatrooms which block open \
				 registration servers."
					.red()
			);
			eprintln!(
				"If you enabled it only for the purpose of creating the first account, {} and \
				 create the first account using the token above.",
				"disable it now, restart Continuwuity,".red(),
			);
			// TODO link to a guide on setting up reCAPTCHA
		}

		if self.services.config.emergency_password.is_some() {
			eprintln!();
			eprintln!(
				"{}",
				"You have set an emergency password for the server user! You almost certainly \
				 do not want to do this."
					.red()
			);
			eprintln!(
				"If you set the password only for the purpose of creating the first account, {} \
				 and create the first account using the token above.",
				"disable it now, restart Continuwuity,".red(),
			);
		}

		eprintln!();
		if std::io::stdin().is_terminal() && self.services.config.admin_console_automatic {
			eprintln!(
				"You may also create the first user through the admin console below using the \
				 `users create-user` command."
			);
		} else {
			eprintln!(
				"If you're running the server interactively, you may also create the first user \
				 through the admin console using the `users create-user` command. Press Ctrl-C \
				 to open the console."
			);
		}
		eprintln!("If you need assistance setting up your homeserver, make a Matrix account on another homeserver and join our chatroom: https://matrix.to/#/#continuwuity:continuwuity.org");

		eprintln!("{}", "============".bold());
	}
}
