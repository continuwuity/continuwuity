use axum::Router;

mod change;
mod reset;

pub(crate) fn build() -> Router<crate::State> {
	#[allow(clippy::wildcard_imports)]
	use self::*;

	Router::new()
		.nest("/change", change::build())
		.nest("/reset/", reset::build())
}
