#[cfg(continuwuity_bench)]
extern crate test;

#[cfg(continuwuity_bench)]
#[cfg_attr(continuwuity_bench, bench)]
fn ser_str(b: &mut test::Bencher) {
	use continuwuity::ruma::{RoomId, UserId};

	use crate::ser::serialize_to_vec;

	let user_id: &UserId = "@user:example.com".try_into().unwrap();
	let room_id: &RoomId = "!room:example.com".try_into().unwrap();
	b.iter(|| {
		let key = (user_id, room_id);
		let _s = serialize_to_vec(key).expect("failed to serialize user_id");
	});
}
