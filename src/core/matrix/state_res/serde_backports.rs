//! These functions are copied from an old version of Ruma. power_levels.rs uses them to lazily deserialize power level events.
//! Upstream Ruma uses a much more elegant approach in its state resolution code, which we may want
//! to look into at some point.

use std::marker::PhantomData;
use std::fmt;

use serde::{Deserialize, Deserializer, de::{MapAccess, Visitor}};
use ruma::{Int, serde::deserialize_v1_powerlevel};

/// Take a Map with values of either an integer number or a string and deserialize
/// those to integer numbers in a Vec of sorted pairs.
///
/// To be used like this:
/// `#[serde(deserialize_with = "vec_deserialize_v1_powerlevel_values")]`
pub fn vec_deserialize_v1_powerlevel_values<'de, D, T>(de: D) -> Result<Vec<(T, Int)>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Ord,
{
    #[repr(transparent)]
    struct IntWrap(Int);

    impl<'de> Deserialize<'de> for IntWrap {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserialize_v1_powerlevel(deserializer).map(IntWrap)
        }
    }

    struct IntMapVisitor<T> {
        _phantom: PhantomData<T>,
    }

    impl<T> IntMapVisitor<T> {
        fn new() -> Self {
            Self { _phantom: PhantomData }
        }
    }

    impl<'de, T> Visitor<'de> for IntMapVisitor<T>
    where
        T: Deserialize<'de> + Ord,
    {
        type Value = Vec<(T, Int)>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a map with integers or strings as values")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut res = Vec::new();
            if let Some(hint) = map.size_hint() {
                res.reserve(hint);
            }

            while let Some((k, IntWrap(v))) = map.next_entry()? {
                res.push((k, v));
            }

            res.sort_unstable();
            res.dedup_by(|a, b| a.0 == b.0);

            Ok(res)
        }
    }

    de.deserialize_map(IntMapVisitor::new())
}

/// Take a Map with integer values and deserialize those to a Vec of sorted pairs
///
/// To be used like this:
/// `#[serde(deserialize_with = "vec_deserialize_int_powerlevel_values")]`
pub fn vec_deserialize_int_powerlevel_values<'de, D, T>(de: D) -> Result<Vec<(T, Int)>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Ord,
{
    struct IntMapVisitor<T> {
        _phantom: PhantomData<T>,
    }

    impl<T> IntMapVisitor<T> {
        fn new() -> Self {
            Self { _phantom: PhantomData }
        }
    }

    impl<'de, T> Visitor<'de> for IntMapVisitor<T>
    where
        T: Deserialize<'de> + Ord,
    {
        type Value = Vec<(T, Int)>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a map with integers as values")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut res = Vec::new();
            if let Some(hint) = map.size_hint() {
                res.reserve(hint);
            }

            while let Some(item) = map.next_entry()? {
                res.push(item);
            }

            res.sort_unstable();
            res.dedup_by(|a, b| a.0 == b.0);

            Ok(res)
        }
    }

    de.deserialize_map(IntMapVisitor::new())
}