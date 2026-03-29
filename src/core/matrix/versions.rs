use std::collections::BTreeMap;

pub fn versions() -> Vec<String> {
    vec![
        "r0.0.1".to_owned(),
        "r0.1.0".to_owned(),
        "r0.2.0".to_owned(),
        "r0.3.0".to_owned(),
        "r0.4.0".to_owned(),
        "r0.5.0".to_owned(),
        "r0.6.0".to_owned(),
        "r0.6.1".to_owned(),
        "v1.1".to_owned(),
        "v1.2".to_owned(),
        "v1.3".to_owned(),
        "v1.4".to_owned(),
        "v1.5".to_owned(),
        "v1.8".to_owned(),
        "v1.11".to_owned(),
        "v1.12".to_owned(),
        "v1.13".to_owned(),
        "v1.14".to_owned(),
    ]
}

pub fn unstable_features() -> BTreeMap<String, bool> {
    BTreeMap::from_iter([
        ("org.matrix.e2e_cross_signing".to_owned(), true),
        ("org.matrix.msc2285.stable".to_owned(), true), /* private read receipts (https://github.com/matrix-org/matrix-spec-proposals/pull/2285) */
        ("uk.half-shot.msc2666.query_mutual_rooms".to_owned(), true), /* query mutual rooms (https://github.com/matrix-org/matrix-spec-proposals/pull/2666) */
        ("org.matrix.msc2836".to_owned(), true), /* threading/threads (https://github.com/matrix-org/matrix-spec-proposals/pull/2836) */
        ("org.matrix.msc2946".to_owned(), true), /* spaces/hierarchy summaries (https://github.com/matrix-org/matrix-spec-proposals/pull/2946) */
        ("org.matrix.msc3026.busy_presence".to_owned(), true), /* busy presence status (https://github.com/matrix-org/matrix-spec-proposals/pull/3026) */
        ("org.matrix.msc3827".to_owned(), true), /* filtering of /publicRooms by room type (https://github.com/matrix-org/matrix-spec-proposals/pull/3827) */
        ("org.matrix.msc3952_intentional_mentions".to_owned(), true), /* intentional mentions (https://github.com/matrix-org/matrix-spec-proposals/pull/3952) */
        ("org.matrix.msc3916.stable".to_owned(), true), /* authenticated media (https://github.com/matrix-org/matrix-spec-proposals/pull/3916) */
        ("org.matrix.msc4180".to_owned(), true), /* stable flag for 3916 (https://github.com/matrix-org/matrix-spec-proposals/pull/4180) */
        ("uk.tcpip.msc4133".to_owned(), true), /* Extending User Profile API with Key:Value Pairs (https://github.com/matrix-org/matrix-spec-proposals/pull/4133) */
        ("us.cloke.msc4175".to_owned(), true), /* Profile field for user time zone (https://github.com/matrix-org/matrix-spec-proposals/pull/4175) */
        ("org.matrix.simplified_msc3575".to_owned(), true), /* Simplified Sliding sync (https://github.com/matrix-org/matrix-spec-proposals/pull/4186) */
        ("uk.timedout.msc4323".to_owned(), true), /* agnostic suspend (https://github.com/matrix-org/matrix-spec-proposals/pull/4323) */
        ("org.matrix.msc4155".to_owned(), true), /* invite filtering (https://github.com/matrix-org/matrix-spec-proposals/pull/4155) */
    ])
}