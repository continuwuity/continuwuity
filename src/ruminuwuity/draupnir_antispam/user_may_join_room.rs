//! `POST /api/1/spam_check/user_may_join_room`
//!
//! Endpoint that checks whether a user may join a given room via Draupnir anti-spam

pub mod v1 {
    use ruma::{
        OwnedRoomId, OwnedUserId, api::{auth_scheme::AppserviceToken, request, response}, metadata
    };

    metadata! {
        method: POST,
        rate_limited: false,
        authentication: AppserviceToken,
        history: {
            1.0 => "/api/1/spam_check/user_may_join_room",
        }
    }

    /// Request type for the `user_may_join_room` callback.
    #[request]
    pub struct Request {
        /// The user trying to join a room
        pub user: OwnedUserId,
        /// The room the user is trying to join
        pub room: OwnedRoomId,
        /// Whether the user was invited to this room
        pub is_invited: bool,
    }

    /// Response type for the `user_may_join_room` callback.
    #[response]
    #[derive(Default)]
    pub struct Response;

    impl Request {
        /// Creates a new empty `Request`.
        #[must_use]
        pub fn new(user: OwnedUserId, room: OwnedRoomId, is_invited: bool) -> Self {
            Self { user, room, is_invited }
        }
    }

    impl Response {
        /// Creates a new empty `Response`.
        #[must_use]
        pub fn new() -> Self {
            Self::default()
        }
    }
}
