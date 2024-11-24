// import fontBoldUrl from './Inter-Bold.ttf?url';
// import fontRegularUrl from './Inter-Regular.ttf?url';
// This is a hack
// Get the URL that the server is running on
// console.log(import.meta.env)
// let base = (import.meta.env.VITE_DOMAIN || "http://localhost:5173") + import.meta.env.BASE_URL;
// if (base?.endsWith('/')) {
//     base = base.slice(0, -1);
// }
// // console.log(base)
// const fontBoldData = await (await fetch(base + fontBoldUrl)).arrayBuffer();
// const fontRegularData = await (await fetch(base + fontRegularUrl)).arrayBuffer();
// import { readFileSync } from 'fs';
// const fontBoldUrl = new URL('./Inter-Bold.ttf', import.meta.url).href
// const fontBoldData = readFileSync(fontBoldUrl);
// const fontRegularUrl = new URL('./Inter-Regular.ttf', import.meta.url).href
// const fontRegularData = readFileSync(fontRegularUrl);
// console.log(fontBoldUrl)
// export { fontBoldData, fontRegularData };