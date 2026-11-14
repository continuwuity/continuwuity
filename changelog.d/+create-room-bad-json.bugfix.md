Creating a room with valid but non-canonical JSON now returns `400 M_BAD_JSON` instead of panicking the worker and returning `500 M_UNKNOWN`.
