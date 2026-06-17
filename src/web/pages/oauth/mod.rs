use axum::Router;

mod grant;

pub(crate) fn build() -> Router<crate::State> {
	#[allow(clippy::wildcard_imports)]
	use self::*;

	Router::new().nest("/grant/", grant::build())
}
