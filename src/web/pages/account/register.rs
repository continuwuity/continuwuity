use std::{collections::BTreeMap, time::SystemTime};

use axum::{
	Extension, Router,
	extract::{Query, State},
	response::{Redirect, Response},
	routing::{get, on},
};
use conduwuit_core::{config::TermsDocument, warn};
use conduwuit_service::{
	mailer::messages,
	registration_tokens::ValidToken,
	uiaa::{FlowStatus, TrustedFlowStatus, UntrustedFlowStatus},
	users::HashedPassword,
};
use futures::FutureExt;
use lettre::{Address, message::Mailbox};
use ruma::{ClientSecret, OwnedClientSecret, OwnedServerName, OwnedSessionId, OwnedUserId};
use serde::{Deserialize, Serialize, de::IgnoredAny};
use tower_sessions::Session;
use validator::{Validate, ValidationError, ValidationErrors};

use crate::{
	WebError,
	extract::{Expect, PostForm},
	pages::{GET_POST, Result, TemplateContext, account::ThreepidQuery},
	response,
	session::{LoginTarget, User, UserSession},
	template,
};

const COMPLETED_REGISTRATION_KEY: &str = "completed_registration";

pub(crate) fn build() -> Router<crate::State> {
	Router::new()
		.route("/", on(GET_POST, route_register))
		.route("/validate", get(get_register_email_validate))
}

template! {
	struct Register use "register.html.j2" {
		server_name: OwnedServerName,
		is_first_run: bool,
		body: RegisterBody
	}
}

#[derive(Debug)]
enum RegisterBody {
	Unavailable,
	UsernamePrompt {
		allow_federation: bool,
		trusted_flow_status: TrustedFlowStatus,
		untrusted_flow_status: UntrustedFlowStatus,
		username_error: Option<String>,
		next: Option<LoginTarget>,
	},
	DetailsPrompt {
		username: Option<String>,
		require_email: bool,
		flow: RegistrationFlowParameters,
		terms: BTreeMap<String, TermsDocument>,
		validation_errors: ValidationErrors,
	},
}

#[derive(Default, Deserialize, Serialize)]
pub(crate) struct RegisterQuery {
	pub username: Option<String>,
	pub token: Option<String>,
	pub flow: Option<RequestedRegistrationFlow>,
	#[serde(default)]
	pub from_landing: bool,
	#[serde(flatten)]
	pub next: Option<LoginTarget>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RequestedRegistrationFlow {
	Untrusted,
	Trusted,
}

#[derive(Debug)]
enum RegistrationFlowParameters {
	Untrusted {
		recaptcha_sitekey: Option<String>,
	},
	Trusted {
		registration_token: Option<String>,
	},
}

#[derive(Deserialize, Validate)]
struct RegistrationForm {
	flow: RequestedRegistrationFlow,
	username: String,
	email: Option<Address>,
	#[validate(length(min = 1, message = "Password cannot be empty"))]
	password: String,
	#[validate(must_match(other = "password", message = "Passwords must match"))]
	confirm_password: String,
	registration_token: Option<String>,
	#[serde(rename = "g-recaptcha-response")]
	recaptcha_response: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct CompletedRegistration {
	user_id: OwnedUserId,
	password_hash: HashedPassword,
	registration_token: Option<ValidToken>,
	next: Option<LoginTarget>,
}

async fn route_register(
	State(services): State<crate::State>,
	Extension(context): Extension<TemplateContext>,
	session_store: Session,
	Expect(Query(query)): Expect<Query<RegisterQuery>>,
	PostForm(form): PostForm<RegistrationForm>,
) -> Result {
	let is_first_run = services.firstrun.is_first_run();

	if services.oidc.enabled()
		|| session_store
			.get::<IgnoredAny>(User::KEY)
			.await
			.unwrap()
			.is_some()
	{
		// Redirect to the account panel if the user is already logged in
		// or OIDC is enabled
		return response!(Redirect::to(&LoginTarget::Account.target_path()));
	}

	let validation_errors = if let Some(form) = form {
		match form.validate() {
			| Ok(()) => {
				match begin_registration(
					&services,
					context.clone(),
					session_store,
					form,
					query.next.clone(),
				)
				.boxed()
				.await?
				{
					| Ok(response) => return Ok(response),
					| Err(err) => err,
				}
			},
			| Err(err) => err,
		}
	} else {
		ValidationErrors::new()
	};

	let FlowStatus {
		trusted: trusted_flow_status,
		untrusted: untrusted_flow_status,
	} = services.uiaa.registration_flow_status().await;

	if matches!(trusted_flow_status, TrustedFlowStatus::Unavailable)
		&& matches!(untrusted_flow_status, UntrustedFlowStatus::Unavailable)
	{
		return response!(Register::new(
			context,
			services.globals.server_name().to_owned(),
			services.firstrun.is_first_run(),
			RegisterBody::Unavailable
		));
	}

	if query.username.is_some() && query.flow.is_none() {
		return response!(WebError::BadRequest(
			"A flow must be provided if a username is provided".to_owned()
		));
	}

	if let Some(username) = &query.username
		&& query.from_landing
	{
		// Check if the username is valid and available before showing the details form
		// to keep the user from wasting their time

		if let Err(err) = services
			.users
			.determine_registration_user_id(Some(username.to_owned()), None, None)
			.await
		{
			return response!(Register::new(
				context,
				services.globals.server_name().to_owned(),
				services.firstrun.is_first_run(),
				RegisterBody::UsernamePrompt {
					allow_federation: services.config.allow_federation,
					trusted_flow_status,
					untrusted_flow_status,
					username_error: Some(err.message()),
					next: query.next,
				}
			));
		}
	}

	let body = {
		let terms = services.config.registration_terms.documents.clone();

		match (query.flow, query.token) {
			| (Some(RequestedRegistrationFlow::Trusted), token) | (_, token @ Some(_)) =>
				RegisterBody::DetailsPrompt {
					username: query.username,
					require_email: services
						.config
						.smtp
						.as_ref()
						.is_some_and(|smtp| smtp.require_email_for_token_registration),
					flow: RegistrationFlowParameters::Trusted { registration_token: token },
					terms,
					validation_errors,
				},
			| (_, token) if is_first_run => RegisterBody::DetailsPrompt {
				username: query.username,
				require_email: false,
				flow: RegistrationFlowParameters::Trusted { registration_token: token },
				terms,
				validation_errors,
			},
			| (Some(RequestedRegistrationFlow::Untrusted), _) => RegisterBody::DetailsPrompt {
				username: query.username,
				require_email: services
					.config
					.smtp
					.as_ref()
					.is_some_and(|smtp| smtp.require_email_for_registration),
				flow: RegistrationFlowParameters::Untrusted {
					recaptcha_sitekey: services.config.recaptcha_site_key.clone(),
				},
				terms,
				validation_errors,
			},
			| (None, None) => RegisterBody::UsernamePrompt {
				allow_federation: services.config.allow_federation,
				trusted_flow_status,
				untrusted_flow_status,
				username_error: None,
				next: query.next,
			},
		}
	};

	response!(Register::new(
		context,
		services.globals.server_name().to_owned(),
		is_first_run,
		body
	))
}

template! {
	struct RegisterEmailValidate use "register_email_validate.html.j2" {
		session_id: OwnedSessionId,
		client_secret: OwnedClientSecret,
		validation_error: bool
	}
}

#[derive(Deserialize, Serialize)]
struct RegisterEmailValidateQuery {
	#[serde(flatten)]
	threepid: ThreepidQuery,
}

async fn get_register_email_validate(
	State(services): State<crate::State>,
	Extension(context): Extension<TemplateContext>,
	session_store: Session,
	Expect(Query(RegisterEmailValidateQuery {
		threepid: ThreepidQuery { client_secret, session_id },
	})): Expect<Query<RegisterEmailValidateQuery>>,
) -> Result {
	let Ok(session) = services
		.threepid
		.get_valid_session(&session_id, &client_secret)
		.await
	else {
		return response!(RegisterEmailValidate::new(context, session_id, client_secret, true));
	};

	let Some(completed_registration) = session_store
		.get::<CompletedRegistration>(COMPLETED_REGISTRATION_KEY)
		.await
		.expect("should be able to deserialize completed session")
	else {
		return response!(WebError::BadRequest(
			"Inapplicable session. What are you doing here?".to_owned()
		));
	};

	let email = session.consume();

	response!(
		complete_registration(&services, session_store, completed_registration, Some(email))
			.await?
	)
}

async fn begin_registration(
	services: &crate::State,
	context: TemplateContext,
	session_store: Session,
	form: RegistrationForm,
	next: Option<LoginTarget>,
) -> Result<Result<Response, ValidationErrors>> {
	let open_registration = services
		.config
		.yes_i_am_very_very_sure_i_want_an_open_registration_server_prone_to_abuse;
	let mut errors = ValidationErrors::new();

	let user_id = match services
		.users
		.determine_registration_user_id(Some(form.username), form.email.as_ref(), None)
		.await
	{
		| Ok(user_id) => user_id,
		| Err(err) => {
			errors.add(
				"username",
				ValidationError::new("invalid").with_message(err.message().into()),
			);
			return Ok(Err(errors));
		},
	};

	let password_hash = match HashedPassword::new(&form.password) {
		| Ok(password) => password,
		| Err(err) => {
			errors.add(
				"password",
				ValidationError::new("invalid").with_message(err.message().into()),
			);
			return Ok(Err(errors));
		},
	};

	let mut registration_token = None;

	// Check flow-specific form fields
	match form.flow {
		| RequestedRegistrationFlow::Trusted => {
			// If the form claims to be using the trusted flow, it has to have a
			// registration token

			let Some(valid_token) = async {
				services
					.registration_tokens
					.validate_token(form.registration_token?)
					.await
			}
			.await
			else {
				errors.add(
					"registration_token",
					ValidationError::new("invalid")
						.with_message("Invalid registration token".into()),
				);
				return Ok(Err(errors));
			};

			registration_token = Some(valid_token);
		},
		| RequestedRegistrationFlow::Untrusted => {
			// Don't check auth for the untrusted flow at all if open reg is enabled
			if !open_registration {
				// If the form claims to be using the untrusted flow, it _may_ need to have a
				// reCAPTCHA response if reCAPTCHA is configured

				if let Some(recaptcha_private_site_key) =
					&services.config.recaptcha_private_site_key
				{
					let Some(recaptcha_response) = form.recaptcha_response else {
						return Err(WebError::BadRequest(
							"reCAPTCHA response expected".to_owned(),
						));
					};

					if recaptcha_verify::verify_v3(
						recaptcha_private_site_key,
						&recaptcha_response,
						None,
					)
					.await
					.is_err()
					{
						errors.add(
							"recaptcha",
							ValidationError::new("missing")
								.with_message("Please complete the CAPTCHA".into()),
						);
						return Ok(Err(errors));
					}
				}
			}
		},
	}

	let completed_registration = CompletedRegistration {
		user_id,
		password_hash,
		registration_token,
		next,
	};

	// Check if we need to send an email
	let require_email = services
		.config
		.smtp
		.as_ref()
		.is_some_and(|smtp| match form.flow {
			| RequestedRegistrationFlow::Trusted => smtp.require_email_for_token_registration,
			| RequestedRegistrationFlow::Untrusted =>
				!open_registration && smtp.require_email_for_registration,
		});

	if require_email {
		// If an email is required we have to validate it before we can complete
		// registration
		let Some(address) = form.email else {
			errors.add(
				"email",
				ValidationError::new("missing")
					.with_message("Please provide an email address".into()),
			);
			return Ok(Err(errors));
		};

		if services
			.threepid
			.get_localpart_for_email(&address)
			.await
			.is_some()
		{
			errors.add(
				"email",
				ValidationError::new("in_use")
					.with_message("This email address is already in use.".into()),
			);
			return Ok(Err(errors));
		}

		let client_secret = ClientSecret::new();

		let session_id = {
			match services
				.threepid
				.send_validation_email(
					Mailbox::new(None, address.clone()),
					|verification_link| messages::NewAccount {
						server_name: services.globals.server_name().as_str(),
						verification_link,
					},
					&client_secret,
					0,
				)
				.await
			{
				| Ok(session_id) => session_id,
				| Err(err) => {
					warn!(
						"Failed to send new account message for {} to {}: {err}",
						&completed_registration.user_id, address,
					);

					errors.add(
						"email",
						ValidationError::new("invalid").with_message(
							"Failed to send validation email. Is this address correct?".into(),
						),
					);
					return Ok(Err(errors));
				},
			}
		};

		session_store
			.insert(COMPLETED_REGISTRATION_KEY, completed_registration)
			.await
			.expect("should have been able to serialize completed registration");

		Ok(response!(
			RegisterEmailValidate::new(context, session_id, client_secret, false,)
		))
	} else {
		// If email isn't required we can immediately complete registration
		Ok(response!(
			complete_registration(services, session_store, completed_registration, None).await?
		))
	}
}

async fn complete_registration(
	services: &crate::State,
	session_store: Session,
	CompletedRegistration {
		user_id,
		password_hash,
		registration_token,
		next,
	}: CompletedRegistration,
	email: Option<Address>,
) -> Result<Redirect> {
	services
		.users
		.create_local_account(&user_id, Some(password_hash), email)
		.await?;

	if let Some(registration_token) = registration_token {
		services
			.registration_tokens
			.mark_token_as_used(registration_token);
	}

	let user_session = UserSession { user_id, last_login: SystemTime::now() };

	session_store
		.insert(User::KEY, user_session)
		.await
		.expect("should be able to serialize user session");

	Ok(Redirect::to(&next.unwrap_or_default().target_path()))
}
