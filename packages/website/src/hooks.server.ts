import type { Handle } from "@sveltejs/kit";

const securityHeaders = {
    'X-Content-Type-Options': 'nosniff',
    'X-XSS-Protection': '0',

    "Referrer-Policy": "no-referrer-when-downgrade",

    "Permissions-Policy": "payment=(), geolocation=(self), fullscreen=(self)",

    'Cross-Origin-Embedder-Policy': 'require-corp',
    'Cross-Origin-Opener-Policy': 'same-origin',
    'Cross-Origin-Resource-Policy': 'same-origin',

}

export const handle: Handle = async ({ event, resolve }) => {
    const response = await resolve(event);
    Object.entries(securityHeaders).forEach(
        ([header, value]) => response.headers.set(header, value)
    );

    response.headers.delete("x-sveltekit-page")

    return response;
}
