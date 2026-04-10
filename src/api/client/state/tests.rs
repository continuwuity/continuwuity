use super::*;

#[test]
fn test_strip_room_member() -> Result<()> {
	//Test setup
	let body = r#"
		{
			"avatar_url": "Something",
			"displayname": "Someone",
			"join_authorized_via_users_server": "@someone:domain.tld",
			"membership": "join"
		}"#;
	println!("JSON (original): {body}");
	let json: &mut Raw<AnyStateEventContent> =
		&mut Raw::<AnyStateEventContent>::from_json_string(body.to_owned())?;
	let mut membership_content: RoomMemberEventContent =
		json.deserialize_as_unchecked::<RoomMemberEventContent>()?;

	//Begin Test
	membership_content.join_authorized_via_users_server = None;
	*json = Raw::<AnyStateEventContent>::from_json_string(serde_json::to_string(
		&membership_content,
	)?)?;

	//Compare result
	let result = json.json().get();
	println!("JSON (modified): {result}");
	assert_eq!(
		result,
		r#"{"avatar_url":"Something","displayname":"Someone","membership":"join"}"#
	);

	Ok(())
}
