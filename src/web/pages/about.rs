use std::collections::BTreeMap;

use axum::{Extension, Router, extract::State, routing::get};
use conduwuit_core::config::TermsDocument;
use ruma::{
	OwnedServerName,
	api::client::discovery::discover_support::{Contact, ContactRole},
};
use url::Url;

use crate::{
	pages::{Result, TemplateContext},
	response, template,
};

pub(crate) fn build() -> Router<crate::State> { Router::new().route("/", get(get_about)) }

template! {
	struct About use "about.html.j2" {
		server_name: OwnedServerName,
		support_page: Option<Url>,
		contacts: Vec<Contact>,
		terms: BTreeMap<String, TermsDocument>
	}
}

async fn get_about(
	State(services): State<crate::State>,
	Extension(context): Extension<TemplateContext>,
) -> Result {
	response!(About::new(
		context,
		services.globals.server_name().to_owned(),
		services.config.well_known.support_page.clone(),
		services.admin.get_support_contacts().await,
		services.config.registration_terms.documents.clone()
	))
}
