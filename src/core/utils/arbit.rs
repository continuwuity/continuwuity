pub const ARBIT_LENGTH: usize = 32;
/// An arbit is an opaque, random 32-byte identifier.
/// They are useful as unique, unordered database keys with no particular
/// semantics except uniqueness.
pub type Arbit = [u8; ARBIT_LENGTH];

#[must_use]
#[inline]
pub fn arbit() -> Arbit {
	let mut arbit = [0; ARBIT_LENGTH];
	fastrand::fill(&mut arbit);
	arbit
}
