use conduwuit::{Result, matrix::pdu::PduCount};

/// Parse a pagination token
pub(crate) fn pagination_token_to_count(token: &str) -> Result<PduCount> { token.parse() }

/// Convert a PduCount to a token string
pub(crate) fn count_to_pagination_token(count: PduCount) -> String { count.to_string() }
