<script lang="ts">
// import { Publisher } from "./peritext/pubsub"
// import type { Change } from "./peritext/micromerge"
// import type { Editor } from "./peritext/bridge"
// import { Mark } from "prosemirror-model"
// import Micromerge from "./peritext/micromerge"

// import {createEditor, initializeDocs} from "./peritext/bridge"

import menu from "./menu";

// import Hero from "$lib/Hero.svelte";
let editorNode: Element;
let changesNode: Element;
let marksNode: Element;
// const publisher = new Publisher<Array<Change>>()

// const editors: { [key: string]: Editor } = {}

// const renderMarks = (domNode: Element, marks: readonly Mark[]): void => {
//     domNode.innerHTML = marks
//         .map(m => `• ${m.type.name} ${Object.keys(m.attrs).length !== 0 ? JSON.stringify(m.attrs) : ""}`)
//         .join("<br/>")
// }

// const aliceDoc = new Micromerge("alice")


// const aliceNode = document.querySelector("#alice")
// const aliceEditor = aliceNode?.querySelector(".editor")
// const aliceChanges = aliceNode?.querySelector(".changes")
// const aliceMarks = aliceNode?.querySelector(".marks")



for (const editor of Object.values(editors)) {
    editor.queue.drop()
}

// Add a button for syncing the two editors
// document.querySelector("#sync")?.addEventListener("click", () => {
//     for (const editor of Object.values(editors)) {
//         editor.queue.flush()
//     }
// })

  // import type { Loro, LoroText } from "loro-crdt";
  import { onMount } from "svelte";

  // import { EditorView } from "prosemirror-view";

  // import { EditorState, TextSelection, Plugin } from "prosemirror-state";
  // import { DOMParser, DOMSerializer, Node, Schema } from "prosemirror-model";
  // import { richTextSchema, EXPAND_CONFIG } from "./prosemirror/schema";

  // import { keymap } from "prosemirror-keymap";
  // import { baseKeymap } from "prosemirror-commands";
  // import { history, redo, undo } from "prosemirror-history";
  import { dropCursor } from "prosemirror-dropcursor";
  import { gapCursor } from "prosemirror-gapcursor";

  // import { richTextKeyMapPlugin } from "./prosemirror/keymap";

  // let loroState: Loro | null = null;
//   onMount(() => {
//     let view: Editor;
//     (async () => {
//       // let b = import(`loro-crdt`);

//       // const Loro = (await b).Loro;
//       // loroState = new Loro();

//     //   let pkgbridge = import(`./peritext/bridge`)
//     // const { createEditor, initializeDocs } = await pkgbridge

    
// initializeDocs(
//     [aliceDoc],
//     [
//         {
//             path: [Micromerge.contentKey],
//             action: "insert",
//             index: 0,
//             values: "This is the Peritext editor demo. Press sync to synchronize the editors. Ctrl-B for bold, Ctrl-i for italic, Ctrl-k for link, Ctrl-e for comment".split(
//                 "",
//             ),
//         },
//         {
//             path: [Micromerge.contentKey],
//             action: "addMark",
//             markType: "strong",
//             startIndex: 84,
//             endIndex: 88,
//         },
//         {
//             path: [Micromerge.contentKey],
//             action: "addMark",
//             markType: "em",
//             startIndex: 100,
//             endIndex: 107,
//         },
//         {
//             path: [Micromerge.contentKey],
//             action: "addMark",
//             markType: "link",
//             attrs: { url: "http://inkandswitch.com" },
//             startIndex: 120,
//             endIndex: 124,
//         },
//         {
//             path: [Micromerge.contentKey],
//             action: "addMark",
//             markType: "comment",
//             attrs: { id: "1" },
//             startIndex: 137,
//             endIndex: 144,
//         },
//     ],
// )

//       view = createEditor({
//         actorId: "alice",
//         editorNode,
//         changesNode,
//         doc: aliceDoc,
//         publisher,
        
//         plugins: [
//           // history(),
//           // keymap({}), // {"Mod-z": undo, "Mod-y": redo, "Mod-Shift-z": redo}
//           // keymap(baseKeymap),
//           dropCursor(),
//           gapCursor(),
//           // richTextKeyMapPlugin,
//           menu,
//           // ...plugins
//         ],
//         editable: true,
//         handleClickOn: (view, pos, node, nodePos, event, direct) => {
//             // Prosemirror calls this once per node that overlaps w/ the clicked pos.
//             // We only want to run our callback once, on the innermost clicked node.
//             if (!direct) return false

//             const marksAtPosition = view.state.doc.resolve(pos).marks()
//             renderMarks(marksNode, marksAtPosition)
//             return false
//         },
//     })
//       // let state = EditorState.create({
//       //   schema: richTextSchema,
//       //   //   doc,
//       //   //   selection,
//       // });

//       // view = new EditorView(editorNode, {
//       //   state,
//       //   nodeViews: {
//       //     // image(node, view, getPos) {
//       //     //   return new ImageView(node, view, getPos);
//       //     // }
//       //   },
//       // });
//     })();

//     return () => {
//       if (view) {
//         view.view.destroy()
//       }
//     };
//   });

</script>

<div class="content card edge">
  <!-- <div>Test</div> -->
  <!-- {#if state}
    <Editor state={state} document={document}/>
    {/if} -->
  <div bind:this={editorNode}></div>
</div>
<div bind:this={changesNode}></div>
<div bind:this={marksNode}></div>

<style>
  .content {
    margin: 48px auto;
    max-width: calc(100% - 2 * var(--spacing));
    width: 520px;
    padding: var(--spacing);
  }

  @media screen and (min-width: 540px) {
    .content {
      margin: 96px auto;
    }
  }

  :global(.ProseMirror) {
    margin: auto;
    outline: none;
  }

  :global(.ProseMirror h1) {
    font-size: 2em;
  }

  :global(.ProseMirror p) {
    font-size: 1em;
    line-height: 1.5em;
  }
</style>
