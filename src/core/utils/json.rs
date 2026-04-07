use std::{fmt, marker::PhantomData, str::FromStr};

use ruma::{CanonicalJsonError, CanonicalJsonObject, canonical_json::to_canonical_value};

use crate::Result;

/// Fallible conversion from any value that implements `Serialize` to a
/// `CanonicalJsonObject`.
///
/// `value` must serialize to an `serde_json::Value::Object`.
pub fn to_canonical_object<T: serde::Serialize>(
	value: T,
) -> Result<CanonicalJsonObject, CanonicalJsonError> {
	use ruma::CanonicalJsonValue;

	match to_canonical_value(value)? {
		| CanonicalJsonValue::Object(map) => Ok(map),
		| _ => Err(to_canonical_value(1.0_f32).unwrap_err()), /* Hack to return a
		                                                       * CanonicalJsonError */
	}
}

pub fn deserialize_from_str<'de, D, T, E>(deserializer: D) -> Result<T, D::Error>
where
	D: serde::de::Deserializer<'de>,
	T: FromStr<Err = E>,
	E: fmt::Display,
{
	struct Visitor<T: FromStr<Err = E>, E>(PhantomData<T>);

	impl<T, Err> serde::de::Visitor<'_> for Visitor<T, Err>
	where
		T: FromStr<Err = Err>,
		Err: fmt::Display,
	{
		type Value = T;

		fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
			write!(formatter, "a parsable string")
		}

		fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
		where
			E: serde::de::Error,
		{
			v.parse().map_err(serde::de::Error::custom)
		}
	}

	deserializer.deserialize_str(Visitor(PhantomData))
}
