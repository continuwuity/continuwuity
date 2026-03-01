use axum::Router;

pub(crate) fn build() -> Router<crate::State> {
	Router::new().nest(
		"/_continuwuity/resources/",
		#[allow(unused_qualifications)]
		memory_serve::load!().index_file(None).into_router(),
	)
}
