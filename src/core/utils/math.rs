mod expected;
mod tried;

use std::{cmp, convert::TryFrom};

pub use checked_ops::checked_ops;

pub use self::{expected::Expected, tried::Tried};
use crate::{Err, Error, Result, debug::type_name, err};

/// Checked arithmetic expression. Returns a Result<R, Error::Arithmetic>
#[macro_export]
#[collapse_debuginfo(yes)]
macro_rules! checked {
	($($input:tt)+) => {
		$crate::utils::math::checked_ops!($($input)+)
			.ok_or_else(|| $crate::err!(Arithmetic("operation overflowed or result invalid")))
	};
}

/// Checked arithmetic expression which panics on failure. This is for
/// expressions which do not meet the threshold for validated! but the caller
/// has no realistic expectation for error and no interest in cluttering the
/// callsite with result handling from checked!.
#[macro_export]
#[collapse_debuginfo(yes)]
macro_rules! expected {
	($msg:literal, $($input:tt)+) => {
		$crate::checked!($($input)+).expect($msg)
	};

	($($input:tt)+) => {
		$crate::expected!("arithmetic expression expectation failure", $($input)+)
	};
}

/// Unchecked arithmetic expression in release-mode. Use for performance when
/// the expression is obviously safe. The check remains in debug-mode for
/// regression analysis.
#[cfg(not(debug_assertions))]
#[macro_export]
#[collapse_debuginfo(yes)]
macro_rules! validated {
	($($input:tt)+) => {
		//#[allow(clippy::arithmetic_side_effects)] {
		//Some($($input)*)
		//	.ok_or_else(|| $crate::err!(Arithmetic("this error should never been seen")))
		//}

		//NOTE: remove me when stmt_expr_attributes is stable
		$crate::expected!("validated arithmetic expression failed", $($input)+)
	};
}

/// Checked arithmetic expression in debug-mode. Use for performance when
/// the expression is obviously safe. The check is elided in release-mode.
#[cfg(debug_assertions)]
#[macro_export]
#[collapse_debuginfo(yes)]
macro_rules! validated {
	($($input:tt)+) => { $crate::expected!($($input)+) }
}

#[inline]
#[allow(clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn usize_from_f64(val: f64) -> Result<usize, Error> {
	// Exclusive upper bound for values whose truncation fits in `usize`.
	// Note that `usize::MAX as f64` rounds up to 2^64 on 64-bit targets, so a
	// strict `<` check against 2^(usize::BITS) rejects all out-of-range values
	// on both 32-bit and 64-bit targets.
	#[allow(clippy::cast_precision_loss)]
	const MAX_USIZE_EXCLUSIVE: f64 = (1_u128 << usize::BITS) as f64;

	if !val.is_finite() {
		return Err!(Arithmetic("Converting non-finite float to unsigned integer"));
	}

	if val < 0.0 {
		return Err!(Arithmetic("Converting negative float to unsigned integer"));
	}

	if val >= MAX_USIZE_EXCLUSIVE {
		return Err!(Arithmetic("Converting out-of-range float to unsigned integer"));
	}

	Ok(val as usize)
}

#[inline]
#[must_use]
pub fn usize_from_ruma(val: ruma::UInt) -> usize {
	usize::try_from(val).expect("failed conversion from ruma::UInt to usize")
}

#[inline]
#[must_use]
pub fn ruma_from_u64(val: u64) -> ruma::UInt {
	ruma::UInt::try_from(val).expect("failed conversion from u64 to ruma::UInt")
}

#[inline]
#[must_use]
pub fn ruma_from_usize(val: usize) -> ruma::UInt {
	ruma::UInt::try_from(val).expect("failed conversion from usize to ruma::UInt")
}

#[inline]
#[must_use]
#[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
pub fn usize_from_u64_truncated(val: u64) -> usize { val as usize }

#[inline]
pub fn try_into<Dst: TryFrom<Src>, Src>(src: Src) -> Result<Dst> {
	Dst::try_from(src).map_err(try_into_err::<Dst, Src>)
}

fn try_into_err<Dst: TryFrom<Src>, Src>(e: <Dst as TryFrom<Src>>::Error) -> Error {
	drop(e);
	err!(Arithmetic(
		"failed to convert from {} to {}",
		type_name::<Src>(),
		type_name::<Dst>()
	))
}

#[inline]
pub fn clamp<T: Ord>(val: T, min: T, max: T) -> T { cmp::min(cmp::max(val, min), max) }

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn usize_from_f64_valid() {
		assert_eq!(usize_from_f64(0.0).unwrap(), 0);
		assert_eq!(usize_from_f64(-0.0).unwrap(), 0);
		assert_eq!(usize_from_f64(42.9).unwrap(), 42);
		assert_eq!(usize_from_f64(1_048_576.0).unwrap(), 1_048_576);
	}

	#[test]
	fn usize_from_f64_rejects_invalid() {
		usize_from_f64(-1.0).unwrap_err();
		usize_from_f64(-0.5).unwrap_err();
		usize_from_f64(f64::NAN).unwrap_err();
		usize_from_f64(f64::INFINITY).unwrap_err();
		usize_from_f64(f64::NEG_INFINITY).unwrap_err();
		usize_from_f64(1e30).unwrap_err();
		#[cfg(target_pointer_width = "64")]
		{
			// `usize::MAX as f64` rounds up to 2^64, which is out of range.
			#[allow(clippy::as_conversions, clippy::cast_precision_loss)]
			let rounded_max = usize::MAX as f64;
			usize_from_f64(rounded_max).unwrap_err();
		}
	}
}
