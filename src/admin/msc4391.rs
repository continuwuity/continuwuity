//! MSC4391 in-room bot commands: describing the admin command tree, and
//! turning structured invocations into argv.

use std::any::TypeId;

use clap::{ArgAction, Command, CommandFactory, builder::ValueParser};
use conduwuit::{Err, Result, err};
use ruma::{
	OwnedEventId, OwnedRoomAliasId, OwnedRoomId, OwnedRoomOrAliasId, OwnedServerName, OwnedUserId,
};
use serde_json::{Map, Value, json};

use crate::admin::AdminCommand;

/// Each leaf admin command's space-separated name and description content.
#[must_use]
pub(crate) fn descriptions() -> Vec<(String, Value)> {
	let mut described = Vec::new();
	walk(&AdminCommand::command(), &mut Vec::new(), &mut described);
	described
}

fn walk(command: &Command, path: &mut Vec<String>, described: &mut Vec<(String, Value)>) {
	for sub in command.get_subcommands() {
		if sub.is_hide_set() {
			continue;
		}
		path.push(sub.get_name().to_owned());
		if sub.has_subcommands() {
			walk(sub, path, described);
		} else {
			let name = path.join(" ");
			described.push((name.clone(), describe(sub, path, &name)));
		}
		path.pop();
	}
}

fn text(body: &impl ToString) -> Value { json!({ "m.text": [{ "body": body.to_string() }] }) }

fn describe(command: &Command, path: &[String], name: &str) -> Value {
	let parent = &path[..path.len().saturating_sub(1)];
	let aliases: Vec<String> = command
		.get_visible_aliases()
		.map(|alias| [parent, &[alias.to_owned()]].concat().join(" "))
		.collect();
	let parameters: Vec<Value> = command
		.get_arguments()
		.filter(|arg| !arg.is_hide_set())
		.filter_map(parameter)
		.collect();

	let mut content = json!({ "command": name, "parameters": parameters });
	if let Some(about) = command.get_about() {
		content["description"] = text(&about);
	}
	if !aliases.is_empty() {
		content["aliases"] = json!(aliases);
	}
	content
}

fn primitive(parser: &ValueParser) -> Value {
	let id = parser.type_id();
	let is = |type_id: TypeId| id == type_id;
	let primitive = |kind: &str| json!({ "schema_type": "primitive", "type": kind });

	if is(TypeId::of::<OwnedRoomOrAliasId>()) {
		return json!({
			"schema_type": "union",
			"variants": [primitive("room_id"), primitive("room_alias")],
		});
	}
	let kind = if is(TypeId::of::<bool>()) {
		"boolean"
	} else if [
		TypeId::of::<u8>(),
		TypeId::of::<u16>(),
		TypeId::of::<u32>(),
		TypeId::of::<u64>(),
		TypeId::of::<usize>(),
		TypeId::of::<i8>(),
		TypeId::of::<i16>(),
		TypeId::of::<i32>(),
		TypeId::of::<i64>(),
		TypeId::of::<isize>(),
	]
	.into_iter()
	.any(is)
	{
		"integer"
	} else if is(TypeId::of::<OwnedUserId>()) {
		"user_id"
	} else if is(TypeId::of::<OwnedRoomId>()) {
		"room_id"
	} else if is(TypeId::of::<OwnedRoomAliasId>()) {
		"room_alias"
	} else if is(TypeId::of::<OwnedEventId>()) {
		"event_id"
	} else if is(TypeId::of::<OwnedServerName>()) {
		"server_name"
	} else {
		"string"
	};
	primitive(kind)
}

fn parameter(arg: &clap::Arg) -> Option<Value> {
	let many = arg
		.get_num_args()
		.is_some_and(|range| range.max_values() > 1);
	let schema = match arg.get_action() {
		| ArgAction::Help | ArgAction::HelpShort | ArgAction::HelpLong | ArgAction::Version =>
			return None,
		| ArgAction::SetTrue | ArgAction::SetFalse =>
			json!({ "schema_type": "primitive", "type": "boolean" }),
		| ArgAction::Count => json!({ "schema_type": "primitive", "type": "integer" }),
		| action => {
			let choices = arg.get_possible_values();
			let item = if choices.is_empty() {
				primitive(arg.get_value_parser())
			} else {
				json!({
					"schema_type": "union",
					"variants": choices
						.iter()
						.filter(|choice| !choice.is_hide_set())
						.map(|choice| json!({
							"schema_type": "literal",
							"literal_type": "string",
							"value": choice.get_name(),
						}))
						.collect::<Vec<_>>(),
				})
			};
			if matches!(action, ArgAction::Append) || many {
				json!({ "schema_type": "array", "items": item })
			} else {
				item
			}
		},
	};

	let mut parameter = json!({ "key": arg.get_id().as_str(), "schema": schema });
	if !arg.is_required_set() {
		parameter["optional"] = json!(true);
	}
	if let Some(help) = arg.get_long_help().or_else(|| arg.get_help()) {
		parameter["description"] = text(&help);
	}
	if let (Some(default), Some(kind)) =
		(arg.get_default_values().first(), parameter["schema"]["type"].as_str())
		&& kind != "boolean"
	{
		let default = default.to_string_lossy();
		parameter["fi.mau.default_value"] = match kind {
			| "integer" => default
				.parse::<i64>()
				.map_or_else(|_| json!(default), |n| json!(n)),
			| _ => json!(default),
		};
	}
	Some(parameter)
}

/// The argv `AdminCommand::try_parse_from` expects for an
/// `org.matrix.msc4391.command` invocation.
pub(crate) fn argv(invocation: &Value) -> Result<Vec<String>> {
	let root = AdminCommand::command();
	let name = invocation
		.get("command")
		.and_then(Value::as_str)
		.ok_or_else(|| err!("The command has no name."))?;
	let empty = Map::new();
	let arguments = match invocation.get("arguments") {
		| None | Some(Value::Null) => &empty,
		| Some(Value::Object(arguments)) => arguments,
		| Some(_) => return Err!("The command's arguments are not an object."),
	};

	let mut argv = vec!["admin".to_owned()];
	let mut command = &root;
	for word in name.split_whitespace() {
		command = command
			.find_subcommand(word)
			.ok_or_else(|| err!("There is no `{name}` command."))?;
		argv.push(command.get_name().to_owned());
	}
	if command.has_subcommands() {
		return Err!("`{name}` needs a subcommand.");
	}

	for key in arguments.keys() {
		if !command
			.get_arguments()
			.filter(|arg| parameter(arg).is_some())
			.any(|arg| arg.get_id() == key)
		{
			return Err!("`{name}` has no `{key}` argument.");
		}
	}

	let mut positionals = Vec::new();
	let mut skipped = None;
	for arg in command.get_arguments() {
		let key = arg.get_id().as_str();
		let value = match arguments.get(key) {
			| None | Some(Value::Null) => {
				if arg.is_positional() {
					skipped.get_or_insert(key);
				}
				continue;
			},
			| Some(value) => value,
		};
		let schema = parameter(arg).expect("described arguments have schemas")["schema"].clone();
		let is_array = schema["schema_type"] == "array";
		let item_schema = if is_array { &schema["items"] } else { &schema };
		let values = match value {
			| Value::Array(items) if is_array => items
				.iter()
				.map(|value| scalar(item_schema, value))
				.collect::<Result<Vec<_>>>()?,
			| Value::Array(_) => return Err!("`{key}` does not accept an array."),
			| _ if is_array => return Err!("`{key}` must be an array."),
			| value => vec![scalar(item_schema, value)?],
		};

		if arg.is_positional() {
			if let Some(skipped) = skipped {
				return Err!("`{key}` can only be given together with `{skipped}`.");
			}
			positionals.extend(values);
			continue;
		}

		let flag = arg
			.get_long()
			.map(|long| format!("--{long}"))
			.or_else(|| arg.get_short().map(|short| format!("-{short}")))
			.ok_or_else(|| err!("`{key}` cannot be passed by name."))?;
		match arg.get_action() {
			| ArgAction::SetTrue => match value {
				| Value::Bool(true) => argv.push(flag),
				| Value::Bool(false) => {},
				| _ => return Err!("`{key}` must be a boolean."),
			},
			| ArgAction::SetFalse => match value {
				| Value::Bool(false) => argv.push(flag),
				| Value::Bool(true) => {},
				| _ => return Err!("`{key}` must be a boolean."),
			},
			| ArgAction::Count => {
				let count = value
					.as_u64()
					.ok_or_else(|| err!("`{key}` must be a count."))?;
				argv.extend((0..count).map(|_| flag.clone()));
			},
			| _ => argv.extend(values.into_iter().map(|value| format!("{flag}={value}"))),
		}
	}

	if !positionals.is_empty() {
		argv.push("--".to_owned());
		argv.extend(positionals);
	}
	Ok(argv)
}

fn scalar(schema: &Value, value: &Value) -> Result<String> {
	match schema["schema_type"].as_str() {
		| Some("union") => schema["variants"]
			.as_array()
			.and_then(|variants| {
				variants
					.iter()
					.find_map(|schema| scalar(schema, value).ok())
			})
			.ok_or_else(|| err!("An argument has a value of an unsupported type.")),
		| Some("literal") if schema["value"] == *value =>
			scalar(&json!({ "schema_type": "primitive", "type": schema["literal_type"] }), value),
		| Some("primitive") => match schema["type"].as_str() {
			| Some("boolean") => value
				.as_bool()
				.map(|value| value.to_string())
				.ok_or_else(|| err!("An argument must be a boolean.")),
			| Some("integer") => value
				.as_i64()
				.or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
				.map(|value| value.to_string())
				.ok_or_else(|| err!("An argument must be an integer.")),
			| Some("room_id") => reference(value, "room_id", &["room_id", "id"]),
			| Some("event_id") => reference(value, "event_id", &["event_id"]),
			| Some(_) => value
				.as_str()
				.map(ToOwned::to_owned)
				.ok_or_else(|| err!("An argument must be a string.")),
			| None => Err!("An argument has an invalid schema."),
		},
		| _ => Err!("An argument has an invalid schema."),
	}
}

fn reference(value: &Value, kind: &str, fields: &[&str]) -> Result<String> {
	let reference = value
		.as_object()
		.ok_or_else(|| err!("A {kind} reference must be an object."))?;
	if reference.get("type").and_then(Value::as_str) != Some(kind) {
		return Err!("A reference must have type `{kind}`.");
	}
	fields
		.iter()
		.find_map(|field| reference.get(*field).and_then(Value::as_str))
		.map(ToOwned::to_owned)
		.ok_or_else(|| err!("A {kind} reference has no id."))
}

#[cfg(test)]
mod tests {
	use std::collections::HashSet;

	use clap::Parser;
	use serde_json::{Value, json};

	use super::{argv, descriptions};
	use crate::admin::AdminCommand;

	#[test]
	fn every_leaf_command_is_described_once_with_unique_parameters() {
		let described = descriptions();
		let names: HashSet<&str> = described.iter().map(|(name, _)| name.as_str()).collect();
		assert_eq!(names.len(), described.len());
		assert!(names.contains("users create"));

		for (name, content) in &described {
			let keys: Vec<&str> = content["parameters"]
				.as_array()
				.unwrap()
				.iter()
				.map(|parameter| parameter["key"].as_str().unwrap())
				.collect();
			let unique: HashSet<&&str> = keys.iter().collect();
			assert_eq!(unique.len(), keys.len(), "{name} repeats a parameter key");
		}
	}

	#[test]
	fn arguments_are_typed_from_the_clap_definitions() {
		let described = descriptions();
		let (_, reset) = described
			.iter()
			.find(|(name, _)| name == "users reset-password")
			.unwrap();
		let parameters = reset["parameters"].as_array().unwrap();
		let find = |key: &str| {
			parameters
				.iter()
				.find(|parameter| parameter["key"] == key)
				.unwrap()
		};

		assert_eq!(find("logout")["schema"]["type"], "boolean");
		assert_eq!(find("logout")["optional"], true);
		assert_eq!(find("username")["schema"]["type"], "string");
		assert!(find("username").get("optional").is_none());
		assert_eq!(find("password")["optional"], true);
	}

	#[test]
	fn a_structured_invocation_parses_as_the_typed_command() {
		let argv = argv(&json!({
			"command": "users reset-password",
			"arguments": { "logout": true, "username": "alice", "password": "-secret" },
		}))
		.unwrap();

		assert_eq!(argv, [
			"admin",
			"users",
			"reset-password",
			"--logout",
			"--",
			"alice",
			"-secret"
		]);
		AdminCommand::try_parse_from(&argv).unwrap();
	}

	#[test]
	fn a_room_reference_passes_its_id() {
		let argv = argv(&json!({
			"command": "rooms moderation ban-room",
			"arguments": { "room": { "type": "room_id", "room_id": "!room:example.org", "via": ["example.org"] } },
		}))
		.unwrap();

		assert_eq!(argv.last().map(String::as_str), Some("!room:example.org"));
		AdminCommand::try_parse_from(&argv).unwrap();
	}

	#[test]
	fn unknown_commands_and_arguments_are_refused() {
		argv(&json!({ "command": "users nope" })).unwrap_err();
		argv(&json!({ "command": "users" })).unwrap_err();
		argv(&json!({ "command": "users create", "arguments": { "nope": 1 } })).unwrap_err();
	}

	#[test]
	fn structured_arguments_match_their_schemas() {
		argv(&json!({
			"command": "users reset-password",
			"arguments": { "logout": "true", "username": "alice" },
		}))
		.unwrap_err();
		argv(&json!({
			"command": "rooms moderation ban-room",
			"arguments": { "room": "!room:example.org" },
		}))
		.unwrap_err();
	}

	fn sample(schema: &Value) -> Value {
		match schema["schema_type"].as_str().unwrap() {
			| "array" => json!([sample(&schema["items"])]),
			| "union" => sample(&schema["variants"][0]),
			| "literal" => schema["value"].clone(),
			| _ => match schema["type"].as_str().unwrap() {
				| "boolean" => json!(true),
				| "integer" => json!(1),
				| "user_id" => json!("@alice:example.org"),
				| "room_id" => json!({ "type": "room_id", "room_id": "!room:example.org" }),
				| "room_alias" => json!("#room:example.org"),
				| "event_id" =>
					json!({ "type": "event_id", "id": "!room:example.org", "event_id": "$event" }),
				| "server_name" => json!("example.org"),
				| _ => json!("mxc://example.org/media"),
			},
		}
	}

	/// Commands whose requirements are "one of these", which MSC4391 cannot
	/// express: every argument is described as optional and clap reports
	/// what is missing.
	const ONE_OF: [&str; 2] = ["token issue", "media delete-url-preview"];

	#[test]
	fn every_described_command_accepts_its_required_arguments() {
		let mut failures = Vec::new();
		for (name, content) in descriptions() {
			if ONE_OF.contains(&name.as_str()) {
				continue;
			}
			let arguments: serde_json::Map<String, Value> = content["parameters"]
				.as_array()
				.unwrap()
				.iter()
				.filter(|parameter| parameter.get("optional").is_none())
				.map(|parameter| {
					(parameter["key"].as_str().unwrap().to_owned(), sample(&parameter["schema"]))
				})
				.collect();
			let invocation = json!({ "command": name, "arguments": arguments });
			match argv(&invocation) {
				| Ok(argv) =>
					if let Err(error) = AdminCommand::try_parse_from(&argv) {
						failures.push(format!("{name}: {argv:?}: {error}"));
					},
				| Err(error) => failures.push(format!("{name}: {error}")),
			}
		}
		assert!(failures.is_empty(), "{}", failures.join("\n"));
	}

	#[test]
	fn literal_schemas_include_their_type() {
		for (_, content) in descriptions() {
			for parameter in content["parameters"].as_array().unwrap() {
				let schema = &parameter["schema"];
				let variants = schema["variants"].as_array().into_iter().flatten();
				for variant in variants.filter(|variant| variant["schema_type"] == "literal") {
					assert_eq!(variant["literal_type"], "string");
				}
			}
		}
	}
}
