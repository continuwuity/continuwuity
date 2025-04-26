use smallstr::SmallString;

pub type StateKey = SmallString<[u8; INLINE_SIZE]>;

const INLINE_SIZE: usize = 48;
