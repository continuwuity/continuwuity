use std::{collections::BTreeSet, hash::Hash};

use itertools::Itertools;
use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct ClientMetadata {
	#[serde(default)]
	pub application_type: ApplicationType,

	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub client_name: Option<String>,

	pub client_uri: Url,

	#[serde(default, deserialize_with = "btreeset_skip_err")]
	pub grant_types: BTreeSet<GrantType>,

	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub logo_uri: Option<Url>,

	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub policy_uri: Option<Url>,

	#[serde(default)]
	pub redirect_uris: Vec<Url>,

	#[serde(default, deserialize_with = "btreeset_skip_err")]
	pub response_types: BTreeSet<ResponseType>,

	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub token_endpoint_auth_method: Option<String>,

	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub tos_uri: Option<Url>,
}

impl ClientMetadata {
	const ACCEPTABLE_LOCALHOSTS: [&str; 3] = ["localhost", "127.0.0.1", "[::1]"];

	pub(super) fn validate(&self) -> Result<(), &'static str> {
		let Some(client_domain) = self.client_uri.domain() else {
			return Err("Client URI must have a domain.");
		};

		if self.client_uri.scheme() != "https" {
			return Err("Client URI must be HTTPS.");
		}

		if !self.client_uri.username().is_empty() || self.client_uri.password().is_some() {
			return Err("Client URI must not include credentials.");
		}

		for uri in [&self.logo_uri, &self.policy_uri, &self.tos_uri]
			.iter()
			.filter_map(|uri| uri.as_ref())
		{
			if uri.scheme() != "https" {
				return Err("All metadata URIs must be HTTPS.");
			}

			if !uri.username().is_empty() || uri.password().is_some() {
				return Err("All metadata URIs must not include credentials.");
			}

			if !uri
				.domain()
				.is_some_and(|domain| is_subdomain(domain, client_domain))
			{
				return Err("All metadata URIs must be subdomains of the client URI.");
			}
		}

		for uri in &self.redirect_uris {
			match uri.scheme() {
				| "https" => {
					// HTTPS URIs are okay for native and web clients

					if !uri.username().is_empty() || uri.password().is_some() {
						return Err("HTTPS redirect URIs must not contain credentials.");
					}
				},
				| "http" if self.application_type == ApplicationType::Native => {
					if uri
						.host_str()
						.is_none_or(|host| !Self::ACCEPTABLE_LOCALHOSTS.contains(&host))
					{
						return Err("HTTP redirect URIs for native applications must only \
						            refer to localhost.");
					}

					if uri.port().is_some() {
						return Err("HTTP redirect URIs for native applications do not need to \
						            specify a port. All ports will be accepted during \
						            authorization.");
					}
				},
				| private_scheme if self.application_type == ApplicationType::Native => {
					let rdns_client_uri = client_domain.split('.').rev().join(".");

					if !private_scheme.starts_with(&rdns_client_uri) {
						return Err("Private-use scheme URIs for native applications must \
						            begin with the application's client URI domain in \
						            reverse-DNS notation.");
					}

					if uri.has_authority() {
						return Err("Private-use scheme URIs for native applications must not \
						            have an authority.");
					}
				},
				| _ =>
					return Err("A redirect URI's scheme is not valid for this application type."),
			}
		}

		Ok(())
	}
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationType {
	#[default]
	Web,
	Native,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantType {
	AuthorizationCode,
	RefreshToken,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseType {
	Code,
}

/// Deserialize a BTreeSet from a sequence, skipping items which fail to
/// deserialize. This is used as a deserialize helper for ClientMetadata to
/// ignore unknown enum variants in a few fields.
fn btreeset_skip_err<'de, D, V>(de: D) -> Result<BTreeSet<V>, D::Error>
where
	D: Deserializer<'de>,
	V: Deserialize<'de> + Hash + Eq + Ord,
{
	use std::marker::PhantomData;

	use serde::de::{SeqAccess, Visitor};

	struct BTreeSetVisitor<V> {
		item: PhantomData<V>,
	}

	impl<'de, V> Visitor<'de> for BTreeSetVisitor<V>
	where
		V: Deserialize<'de> + Hash + Eq + Ord,
	{
		type Value = BTreeSet<V>;

		fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
			write!(formatter, "a sequence")
		}

		fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
		where
			A: SeqAccess<'de>,
		{
			let mut set = BTreeSet::new();

			while let Some(element) = seq.next_element().transpose() {
				if let Ok(element) = element {
					set.insert(element);
				}
			}

			Ok(set)
		}
	}

	de.deserialize_seq(BTreeSetVisitor { item: PhantomData })
}

fn is_subdomain(subdomain: &str, domain: &str) -> bool {
	if subdomain == domain {
		return true;
	}

	subdomain.ends_with(&format!(".{domain}"))
}
