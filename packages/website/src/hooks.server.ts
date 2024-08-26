import {sequence} from '@sveltejs/kit/hooks';
import * as Sentry from '@sentry/sveltekit';
import type { Handle } from "@sveltejs/kit";

Sentry.init({
    dsn: "https://d006c73cc53783930a1521a68ae1c312@o4507835405369344.ingest.de.sentry.io/4507835410481232",
    tracesSampleRate: 1
})

const securityHeaders = {
    'X-Content-Type-Options': 'nosniff',
    'X-XSS-Protection': '0',

    "Referrer-Policy": "no-referrer-when-downgrade",

    "Permissions-Policy": "payment=(), geolocation=(self), fullscreen=(self)",

    'Cross-Origin-Embedder-Policy': 'require-corp',
    'Cross-Origin-Opener-Policy': 'same-origin',
    'Cross-Origin-Resource-Policy': 'same-origin',

    'Report-To': '{"group":"csp-endpoint","max_age":10886400,"endpoints":[{"url":"https://o4507835405369344.ingest.de.sentry.io/api/4507835410481232/security/?sentry_key=d006c73cc53783930a1521a68ae1c312"}],"include_subdomains":true}',
}

export const handle: Handle = sequence(Sentry.sentryHandle(), async ({ event, resolve }) => {
    const response = await resolve(event);
    Object.entries(securityHeaders).forEach(
        ([header, value]) => {
            if (!response.headers.has(header)) {
                response.headers.set(header, value)
            }
        }
    );

    response.headers.delete("x-sveltekit-page")

    return response;
})
export const handleError = Sentry.handleErrorWithSentry();