(<any>window).dataLayer = (<any>window).dataLayer || [];

export function gtag(...args: any[]) {
    (<any>window).dataLayer.push(arguments);
}

gtag('js', new Date());

gtag('config', 'G-Q2R5PQL59Z');