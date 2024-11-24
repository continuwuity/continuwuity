const rootDomain = process.env.VITE_DOMAIN; // or your server IP for dev
import { SENTRY_HOST } from './src/lib/config.js';
import { SENTRY_REPORT_URL } from './src/lib/config.js';

const self = "'self'";
const none = "'none'";
/**
 * @type {import("@sveltejs/kit").CspDirectives}
 */
const cspDirectives = {
    'base-uri': [self],
    'child-src': [self, "blob:"],
    'connect-src': [self, "https://*.google-analytics.com", "https://" + SENTRY_HOST],
    // 'connect-src': [self, 'ws://localhost:*', 'https://hcaptcha.com', 'https://*.hcaptcha.com'],
    'img-src': [self, 'data:',
        'https://*.googletagmanager.com'],
    'font-src': [self, 'data:'],
    'form-action': [self],
    'frame-ancestors': [self],
    'frame-src': [
        self,
        // "https://*.stripe.com",
        // "https://*.facebook.com",
        // "https://*.facebook.net",
        // 'https://hcaptcha.com',
        // 'https://*.hcaptcha.com',
    ],
    'manifest-src': [self],
    'media-src': [self, 'data:'],
    'object-src': [none],
    'style-src': [self, "unsafe-inline"],
    // 'style-src': [self, "'unsafe-inline'", 'https://hcaptcha.com', 'https://*.hcaptcha.com'],
    'default-src': [
        'self',
        ...(rootDomain ? [rootDomain, `ws://${rootDomain}`] : []),
        // 'https://*.google.com',
        // 'https://*.googleapis.com',
        // 'https://*.firebase.com',
        // 'https://*.gstatic.com',
        // 'https://*.cloudfunctions.net',
        // 'https://*.algolia.net',
        // 'https://*.facebook.com',
        // 'https://*.facebook.net',
        // 'https://*.stripe.com',
        // 'https://*.sentry.io',
    ],
    'script-src': [
        self,
        // "unsafe-inline", // chrome suggestion
        'https://*.googletagmanager.com'
        // 'https://*.stripe.com',
        // 'https://*.facebook.com',
        // 'https://*.facebook.net',
        // 'https://hcaptcha.com',
        // 'https://*.hcaptcha.com',
        // 'https://*.sentry.io',
        // 'https://polyfill.io',
    ],
    'worker-src': [self, "blob:"],
    // remove report-to & report-uri if you do not want to use Sentry reporting
    'report-to': ["csp-endpoint"],
    'report-uri': [
        SENTRY_REPORT_URL,
    ],
};


export default cspDirectives;