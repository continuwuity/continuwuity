import { sequence } from '@sveltejs/kit/hooks';
import {init as initSentry, handleErrorWithSentry, sentryHandle} from '@sentry/sveltekit';
import type { Handle } from "@sveltejs/kit";
import { randomBytes } from 'crypto';

initSentry({
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

export const handle: Handle = async (input) => {
    const sentryNonce = randomBytes(16).toString('hex');
    return await sequence(
        sentryHandle({
            // injectFetchProxyScript: false,
            fetchProxyScriptNonce: sentryNonce,
        }),
        async ({ event, resolve }) => {
            const response = await resolve(event);
            let csp = response.headers.get("Content-Security-Policy");
            if (csp) {
                response.headers.set("Content-Security-Policy", csp.replace("script-src", "script-src 'nonce-" + sentryNonce + "'"));
            }

            Object.entries(securityHeaders).forEach(
                ([header, value]) => {
                    if (!response.headers.has(header)) {
                        response.headers.set(header, value)
                    }
                }
            );

            response.headers.delete("x-sveltekit-page")

            return response;
        }
    )(input)
}
export const handleError = handleErrorWithSentry();