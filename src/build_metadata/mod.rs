pub mod built {
	include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

pub static GIT_COMMIT_HASH: Option<&str> = option_env!("GIT_COMMIT_HASH");

pub static GIT_COMMIT_HASH_SHORT: Option<&str> = option_env!("GIT_COMMIT_HASH_SHORT");

// this would be a lot better if Option::or was const.
pub static VERSION_EXTRA: Option<&str> =
	if let v @ Some(_) = option_env!("CONTINUWUITY_VERSION_EXTRA") {
		v
	} else if let v @ Some(_) = option_env!("CONDUWUIT_VERSION_EXTRA") {
		v
	} else if let v @ Some(_) = option_env!("CONDUIT_VERSION_EXTRA") {
		v
	} else {
		GIT_COMMIT_HASH_SHORT
	};
pub static GIT_REMOTE_WEB_URL: Option<&str> = option_env!("GIT_REMOTE_WEB_URL");
pub static GIT_REMOTE_COMMIT_URL: Option<&str> = option_env!("GIT_REMOTE_COMMIT_URL");

// TODO: Mark dirty builds within the version string
