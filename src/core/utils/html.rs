use std::fmt;

/// Wrapper struct which will emit the HTML-escaped version of the contained
/// string when passed to a format string.
pub struct Escape<'a>(pub &'a str);

/// Copied from librustdoc:
/// * <https://github.com/rust-lang/rust/blob/cbaeec14f90b59a91a6b0f17fc046c66fa811892/src/librustdoc/html/escape.rs>
#[allow(clippy::string_slice)]
impl fmt::Display for Escape<'_> {
	fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
		// Because the internet is always right, turns out there's not that many
		// characters to escape: http://stackoverflow.com/questions/7381974
		let Escape(s) = *self;
		let pile_o_bits = s;
		let mut last = 0;
		for (i, ch) in s.char_indices() {
			let s = match ch {
				| '>' => "&gt;",
				| '<' => "&lt;",
				| '&' => "&amp;",
				| '\'' => "&#39;",
				| '"' => "&quot;",
				| _ => continue,
			};

			fmt.write_str(&pile_o_bits[last..i])?;
			fmt.write_str(s)?;

			// NOTE: we only expect single byte characters here - which is fine as long as
			// we only match single byte characters
			last = i.saturating_add(1);
		}

		if last < s.len() {
			fmt.write_str(&pile_o_bits[last..])?;
		}
		Ok(())
	}
}
