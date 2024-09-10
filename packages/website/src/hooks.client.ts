import { SENTRY_DSN } from '$lib/config';
import { init as initSentry, handleErrorWithSentry, makeBrowserOfflineTransport, makeFetchTransport, feedbackIntegration } from '@sentry/sveltekit';

initSentry({
    dsn: SENTRY_DSN,
    environment: import.meta.env.MODE,
    tracesSampleRate: 1.0,

    // This sets the sample rate to be 10%. You may want this to be 100% while
    // in development and sample at a lower rate in production
    replaysSessionSampleRate: 0.0,

    // If the entire session is not sampled, use the below sample rate to sample
    // sessions when an error occurs.
    replaysOnErrorSampleRate: 1.0,

    integrations: [feedbackIntegration({
        autoInject: false,
    })],

    // To enable offline events caching, use makeBrowserOfflineTransport to wrap
    // existing transports and queue events using the browsers' IndexedDB storage
    transport: makeBrowserOfflineTransport(makeFetchTransport),
});

// If you have a custom error handler, pass it to `handleErrorWithSentry`
export const handleError = handleErrorWithSentry();
