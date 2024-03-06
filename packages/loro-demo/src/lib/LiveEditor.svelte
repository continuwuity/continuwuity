<script lang="ts">
    import type { Loro, LoroText } from "loro-crdt";
    import { onMount } from "svelte";

    
  import { EditorView } from "prosemirror-view";


import { EditorState, TextSelection, Plugin } from "prosemirror-state";
import { DOMParser, DOMSerializer, Node, Schema } from "prosemirror-model";
import { richTextSchema, EXPAND_CONFIG } from "./prosemirror/schema";

import { keymap } from "prosemirror-keymap";
import { baseKeymap } from "prosemirror-commands";
import { history, redo, undo } from "prosemirror-history";
import { dropCursor } from "prosemirror-dropcursor"
import { gapCursor } from "prosemirror-gapcursor"

import { richTextKeyMapPlugin } from "./prosemirror/keymap";

import menu from "./menu";

    // import Hero from "$lib/Hero.svelte";
    let editorNode: Element;
    let loroState: Loro | null = null;
	onMount(() => {
        let view: EditorView;
        (async () => {
            let b = import(`loro-crdt`);

            const Loro = (await b).Loro
            loroState = new Loro();

            
    
    let state = EditorState.create({
      schema: richTextSchema,
    //   doc,
    //   selection,
      plugins: [
        // history(),
        keymap({}), // {"Mod-z": undo, "Mod-y": redo, "Mod-Shift-z": redo}
        keymap(baseKeymap),
        dropCursor(),
        gapCursor(),
        richTextKeyMapPlugin,
        menu
        // ...plugins
      ]});

      view = new EditorView(editorNode, {
      state,
      nodeViews: {
        // image(node, view, getPos) {
        //   return new ImageView(node, view, getPos);
        // }
      }
    });
        })();
        
		return () => {
            if (view) {
                view.destroy()
            }
		};
	});
</script>


<div class="content card edge">
    <!-- <div>Test</div> -->
    <!-- {#if state}
    <Editor state={state} document={document}/>
    {/if} -->
    <div bind:this={editorNode}></div>
</div>


<style>
.content {
    margin: 48px auto;
    max-width: calc(100% - 2*var(--spacing));
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
