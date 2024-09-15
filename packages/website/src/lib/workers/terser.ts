import type { MinifyOptions, MinifyOutput } from "terser";
import { receiveMessageData, sendMessageData } from "./util";

const is_browser = typeof window !== "undefined";
export function init() {

    let worker: SharedWorker;
    let currentId = 0;
    let terserModule: typeof import("terser");
    const promises: { [id: number]: [(value: MinifyOutput | PromiseLike<MinifyOutput>) => void, (reason?: any) => void] } = {};
    return {
        minify: async function minify(files: string | string[] | {
            [file: string]: string;
        }, options?: MinifyOptions): Promise<MinifyOutput> {

            if (is_browser && !!window.SharedWorker) {
                if (!worker) {
                    worker = new SharedWorker(new URL('./terserWorker.ts', import.meta.url), { type: "module" })
                    worker.port.onmessage = (e: MessageEvent<any>) => {
                        // invoke the promise's resolve() or reject() depending on whether there was an error.
                        promises[e.data[receiveMessageData.MessageId]][e.data[receiveMessageData.MessageType]](e.data[receiveMessageData.Return]);

                        // ... then delete the promise controller
                        delete promises[e.data[receiveMessageData.MessageId]];

                    }
                }
                worker.port.start()
                return new Promise((resolve, reject) => {
                    promises[++currentId] = [resolve, reject];

                    const data = {
                        [sendMessageData.MessageId]: currentId,
                        [sendMessageData.Parameters]: [files, options
                        ]
                    }
                    worker.port.postMessage(data)
                });

            } else if (is_browser) {
                if (!terserModule) {
                    terserModule = await import("terser")
                }
                return await terserModule.minify(files, options)
            } else {
                return {}
            }
        }
    }

}