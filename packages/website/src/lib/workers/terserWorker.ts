import { minify, type MinifyOptions } from "terser";
import { recieveMessageTypes, sendMessageData } from "./util";

/// <reference lib="sharedworker " />
declare var self: SharedWorkerGlobalScope;

self.onconnect = function (event) {
    const port = event.ports[0];
    port.onmessage = function (e: MessageEvent<{
        [sendMessageData.MessageId]: number,
        [sendMessageData.Parameters]: [string | string[] | {
            [file: string]: string;
        }, MinifyOptions?
        ]
    }>) {
        minify(...e.data[sendMessageData.Parameters]).then(
            // success handler - callback(id, SUCCESS(0), result)
            // if `d` is transferable transfer zero-copy
            d => {

                port.postMessage([e.data[0], recieveMessageTypes.RESOLVE, d],
                    // @ts-ignore
                    [d].filter(x => (
                        (x instanceof ArrayBuffer) ||
                        (x instanceof MessagePort)
                        // || (self.ImageBitmap && x instanceof ImageBitmap)
                    )));
            },
            // error handler - callback(id, ERROR(1), error)
            er => { postMessage([e.data[0], recieveMessageTypes.REJECT, '' + er]); }
        );
    };


}; 