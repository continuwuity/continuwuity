import { init as initSentry, handleErrorWithSentry, makeBrowserOfflineTransport, makeFetchTransport, replayIntegration } from '@sentry/sveltekit';

initSentry({
    dsn: import.meta.env.SENTRY_DSN,
    environment: import.meta.env.MODE,
    tracesSampleRate: 1.0,

    // This sets the sample rate to be 10%. You may want this to be 100% while
    // in development and sample at a lower rate in production
    replaysSessionSampleRate: 0.0,

    // If the entire session is not sampled, use the below sample rate to sample
    // sessions when an error occurs.
    replaysOnErrorSampleRate: 1.0,

    // If you don't want to use Session Replay, just remove the line below:
    integrations: [replayIntegration()],

    // To enable offline events caching, use makeBrowserOfflineTransport to wrap
    // existing transports and queue events using the browsers' IndexedDB storage
    transport: makeBrowserOfflineTransport(makeFetchTransport),
});

// If you have a custom error handler, pass it to `handleErrorWithSentry`
export const handleError = handleErrorWithSentry();
