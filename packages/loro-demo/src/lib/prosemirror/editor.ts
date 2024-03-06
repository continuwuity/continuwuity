// import type { Delta, Loro, LoroText } from "loro-crdt";

// import { EditorState, TextSelection, Plugin } from "prosemirror-state";
// import { DOMParser, DOMSerializer, Node, Schema } from "prosemirror-model";
// import { richTextSchema, EXPAND_CONFIG } from "./schema";

// import { keymap } from "prosemirror-keymap";
// import { baseKeymap } from "prosemirror-commands";
// import { history, redo, undo } from "prosemirror-history";
// import { dropCursor } from "prosemirror-dropcursor"
// import { gapCursor } from "prosemirror-gapcursor"

// import { richTextKeyMapPlugin } from "./keymap";


// import { PluginKey } from 'prosemirror-state' 
// const syncPluginKey = new PluginKey('loro-sync')

// /**
//  * Create an empty editor state with rich text editing capabilities
//  * @param html {string}
//  * @param plugins {array<Plugin>}
//  * @return {EditorState}
//  */
// export const createRichTextEditor = (document: string, state: Loro, plugins = []) => {
//     state.configTextStyle(EXPAND_CONFIG)


//     const syncPlugin = new Plugin({
//         key: syncPluginKey,
//         state: {
//             init(config, instance) {
//                 let richtext = state.getText(document);
//                 richtext.
//                 instance.doc = new Node()
//                 richtext.subscribe(state, (event) => {
//                   for (const change of event.events) {
//                       console.log(change)
//                   }
//                   //   if (!event.local && event.diff.type == "text") {
//                   //     console.log(state.peerId, "CRDT_EVENT", event);
//                   //     const eventDelta = event.diff.diff;
//                   //     const delta: Delta<string>[] = [];
//                   //     let index = 0;
//                   //     for (let i = 0; i < eventDelta.length; i++) {
//                   //       const d = eventDelta[i];
//                   //       const length = d.delete || d.retain || d.insert!.length;
//                   //       // skip the last newline that quill automatically appends
//                   //       if (
//                   //         d.insert &&
//                   //         d.insert === "\n" &&
//                   //         // index === quill.getLength() - 1 &&
//                   //         i === eventDelta.length - 1 &&
//                   //         d.attributes != null &&
//                   //         Object.keys(d.attributes).length > 0
//                   //       ) {
//                   //         delta.push({
//                   //           retain: 1,
//                   //           attributes: d.attributes,
//                   //         });
//                   //         index += length;
//                   //         continue;
//                   //       }
            
//                   //       delta.push(d);
//                   //       index += length;
//                   //     }
            
//                   //     // quill.updateContents(new Delta(delta), "this" as any);
//                   //     // const a = this.richtext.toDelta();
//                   //     // const b = this.quill.getContents().ops;
//                   //     // console.log(this.doc.peerId, "COMPARE AFTER CRDT_EVENT");
//                   //     // if (!assertEqual(a, b as any)) {
//                   //     //   quill.setContents(new Delta(a), "this" as any);
//                   //     // }
//                   //   }
//                 });
//             },
//             apply(tr, value, oldState, newState) {
//                 console.log(tr, value, oldState, newState)
//             },
//         }
//     });
    
//     return EditorState.create({
//       schema: richTextSchema,
//     //   doc,
//     //   selection,
//       plugins: [
//         // history(),
//         keymap({}), // {"Mod-z": undo, "Mod-y": redo, "Mod-Shift-z": redo}
//         keymap(baseKeymap),
//         dropCursor(),
//         gapCursor(),
//         syncPlugin,
//         richTextKeyMapPlugin,
//         ...plugins
//       ]
//     });
//   }