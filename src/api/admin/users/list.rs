use std::collections::BTreeMap;

use axum::extract::State;
use conduwuit::{
	Err,
	utils::{TryFutureExtExt, stream::WidebandExt},
};
use futures::StreamExt;
use ruminuwuity::admin::continuwuity::users::list::v1::{QueryFilter, Request, Response, User};
use service::users::AccountStatus;

use crate::router::Ruma;

/// # `PUT /_continuwuity/admin/v1/users`
///
/// Lists users on this homeserver
pub(crate) async fn list_users(
	State(services): State<crate::State>,
	body: Ruma<Request>,
) -> conduwuit::Result<Response> {
	if !services
		.users
		.is_admin(body.identity.expect_sender_user()?)
		.await
	{
		return Err!(Request(Forbidden("Only server administrators can use this endpoint")));
	}

	let limit = body.limit.unwrap_or(100);
	let offset = limit.saturating_mul(body.page.unwrap_or_default());
	let users = services
		.users
		.stream_local_users()
		.wide_filter_map(|user_id| {
			let body = &body;
			async move {
				let status = services.users.status(&user_id).await;
				assert_ne!(
					status,
					AccountStatus::NotFound,
					"Not found users should not appear in stream_local_users"
				);
				let (deactivated, locked, suspended, admin) = tokio::join!(
					std::future::ready(status == AccountStatus::Deactivated),
					services.users.is_locked(&user_id).unwrap_or_default(),
					services.users.is_suspended(&user_id).unwrap_or_default(),
					services.users.is_admin(&user_id),
				);
				// Exclude deactivated accounts by default
				if !deactivated && body.active == Some(QueryFilter::Exclude) {
					return None;
				}
				if deactivated
					&& body
						.deactivated
						.as_ref()
						.is_none_or(|d| *d == QueryFilter::Include)
				{
					return None;
				}
				// Exclude locked accounts by default
				if locked && body.locked != Some(QueryFilter::Include) {
					return None;
				}
				// Include suspended accounts by default
				if suspended && body.suspended == Some(QueryFilter::Exclude) {
					return None;
				}
				// Include admins by default
				if admin && body.admin == Some(QueryFilter::Exclude) {
					return None;
				}

				Some((user_id, deactivated, suspended, locked, admin))
			}
		})
		.skip(offset)
		.wide_then(|(user_id, deactivated, suspended, locked, admin)| async move {
			let (displayname, avatar_url) = tokio::join!(
				services.users.displayname(&user_id).ok(),
				services.users.avatar_url(&user_id).ok(),
			);
			let user = User {
				displayname,
				avatar_url,
				suspended,
				locked,
				deactivated,
				admin,
			};

			(user_id, user)
		})
		.take(limit)
		.collect::<BTreeMap<_, _>>()
		.await;

	Ok(Response::new(users))
}
