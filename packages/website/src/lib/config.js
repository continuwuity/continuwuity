
// Sentry config
export const SENTRY_HOST = "o4507835405369344.ingest.de.sentry.io"
export const SENTRY_PROJECT_ID = "4507835410481232"
export const SENTRY_KEY = "d006c73cc53783930a1521a68ae1c312"
export const SENTRY_TUNNEL_ALLOWED_IDS = [SENTRY_PROJECT_ID]
export const SENTRY_DSN = "https://" + SENTRY_KEY + "@" + SENTRY_HOST + "/" + SENTRY_PROJECT_ID
export const SENTRY_REPORT_URL = "https://" + SENTRY_HOST + "/api/" + SENTRY_PROJECT_ID + "/security/?sentry_key=" + SENTRY_KEY
