export type FunctionMap = { [x: string]: Function }

export enum sendMessageData {
    MessageId = 0,
    Function = 1,
    Parameters = 2
}

export interface sendMessageMap <T> {
    [sendMessageData.MessageId]: number,
    [sendMessageData.Function]: number,
    [sendMessageData.Parameters]: T[],
}

export enum receiveMessageTypes {
    RESOLVE = 0, // OK
    REJECT = 1 // ERROR
}


export enum receiveMessageData {
    MessageId = 0,
    MessageType = 1,
    Return = 2
}

export interface receiveMessageMap <T> {
    [receiveMessageData.MessageId]: number,
    [receiveMessageData.MessageType]: receiveMessageTypes,
    [receiveMessageData.Return]: T,
}


// // worker
// import { receiveMessageTypes, type FunctionMap } from "./util";

// function makeMessageHandler(functions: FunctionMap) {

//     return (e) => {
//         // Invoking within then() captures exceptions in the supplied async function as rejections
//         Promise.resolve(e.data[1]).then(
//             v => $$.apply($$, v)
//         ).then(
//             // success handler - callback(id, SUCCESS(0), result)
//             // if `d` is transferable transfer zero-copy
//             d => {
//                 postMessage([e.data[0], receiveMessageTypes.SUCCESS, d], [d].filter(x => (
//                     (x instanceof ArrayBuffer) ||
//                     (x instanceof MessagePort) ||
//                     (self.ImageBitmap && x instanceof ImageBitmap)
//                 )));
//             },
//             // error handler - callback(id, ERROR(1), error)
//             er => { postMessage([e.data[0], receiveMessageTypes.ERROR, '' + er]); }
//         );
//     }
// }

// // host
// import { receiveMessageData, receiveMessageTypes, sendMessageData, type receiveMessageMap, type sendMessageMap } from "./util";

// function makeHostHandler(worker: Worker) {

//     let currentId = 0;

//     // Outward-facing promises store their "controllers" (`[request, reject]`) here:
//     const promises: { [id: number]: { [t: number]: IArguments } } = {}
//     ;
//         /** Handle RPC results/errors coming back out of the worker.
//          *  Messages coming from the worker take the form `[id, status, result]`:
//          *    id     - counter-based unique ID for the RPC call
//          *    status - 0 for success, 1 for failure
//          *    result - the result or error, depending on `status`
//          */
//         worker.onmessage = (e: MessageEvent<receiveMessageMap>) => {
//             // invoke the promise's resolve() or reject() depending on whether there was an error.
//             promises[e.data[0]][e.data[1]](e.data[2]);
    
//             // ... then delete the promise controller
//             promises[e.data[0]] = null;
//         };
    
//         // Return a proxy function that forwards calls to the worker & returns a promise for the result.
//         return function () {
//             let args = [].slice.call(arguments);
//             return new Promise(function () {
//                 // Add the promise controller to the registry
//                 promises[++currentId] = arguments;
    
//                 // Send an RPC call to the worker - call(id, params)
//                 // The filter is to provide a list of transferables to send zero-copy
//                 let data: sendMessageMap<any> = {
//                     [sendMessageData.MessageId]: currentId,
//                     [sendMessageData.Function]: 1,
//                     [sendMessageData.Parameters]: args
//                 }
//                 worker.postMessage(data, args.filter(x => (
//                     (x instanceof ArrayBuffer) ||
//                     (x instanceof MessagePort) ||
//                     (self.ImageBitmap && x instanceof ImageBitmap)
//                 )));
//             });
//         };

// }
