use sha2::{Digest, Sha256};

pub type DigestOut = [u8; 256 / 8];

/// Sha256 hash (input gather joined by 0xFF bytes)
#[must_use]
#[tracing::instrument(skip(inputs), level = "trace")]
pub fn delimited<'a, T, I>(mut inputs: I) -> DigestOut
where
	I: Iterator<Item = T> + 'a,
	T: AsRef<[u8]> + 'a,
{
	let mut ctx = Sha256::new();
	if let Some(input) = inputs.next() {
		ctx.update(input.as_ref());
		for input in inputs {
			ctx.update(b"\xFF");
			ctx.update(input.as_ref());
		}
	}

	ctx.finalize()
		.try_into()
		.expect("failed to return Digest buffer")
}

/// Sha256 hash (input gather)
#[must_use]
#[tracing::instrument(skip(inputs), level = "trace")]
pub fn concat<'a, T, I>(inputs: I) -> DigestOut
where
	I: Iterator<Item = T> + 'a,
	T: AsRef<[u8]> + 'a,
{
	inputs
		.fold(Sha256::new(), |mut ctx, input| {
			ctx.update(input.as_ref());
			ctx
		})
		.finalize()
		.try_into()
		.expect("failed to return Digest buffer")
}

/// Sha256 hash
#[inline]
#[must_use]
#[tracing::instrument(skip(input), level = "trace")]
pub fn hash<T>(input: T) -> DigestOut
where
	T: AsRef<[u8]>,
{
	Sha256::digest(input)
		.try_into()
		.expect("failed to return Digest buffer")
}
