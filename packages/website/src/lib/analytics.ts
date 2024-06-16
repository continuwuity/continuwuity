const is_browser = typeof window !== "undefined";

if (is_browser) {
    (<any>window).dataLayer = (<any>window).dataLayer || [];
}

export function gtag(...args: any[]) {
    if (is_browser) {
        (<any>window).dataLayer.push(arguments);
    }
}

gtag('js', new Date());

gtag('config', 'G-Q2R5PQL59Z');