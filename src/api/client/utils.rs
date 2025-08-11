use conduwuit::{
	Result, err,
	matrix::pdu::{PduCount, ShortEventId},
};

/// Parse a pagination token, trying ShortEventId first, then falling back to
/// PduCount
pub(crate) fn parse_pagination_token(token: &str) -> Result<PduCount> {
	// Try parsing as ShortEventId first
	if let Ok(shorteventid) = token.parse::<ShortEventId>() {
		// ShortEventId maps directly to a PduCount in our database
		Ok(PduCount::Normal(shorteventid))
	} else if let Ok(count) = token.parse::<u64>() {
		// Fallback to PduCount for backwards compatibility
		Ok(PduCount::Normal(count))
	} else if let Ok(count) = token.parse::<i64>() {
		// Also handle negative counts for backfilled events
		Ok(PduCount::from_signed(count))
	} else {
		Err(err!(Request(InvalidParam("Invalid pagination token"))))
	}
}

/// Convert a PduCount to a token string (using the underlying ShortEventId)
pub(crate) fn count_to_token(count: PduCount) -> String {
	// The PduCount's unsigned value IS the ShortEventId
	count.into_unsigned().to_string()
}
