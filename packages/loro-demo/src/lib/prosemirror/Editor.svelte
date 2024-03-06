<!-- <script lang="ts">
    import { onMount, onDestroy, createEventDispatcher } from 'svelte'
    // import { createSingleLineEditor } from './state'
    import { EditorView } from "prosemirror-view"
    import { EditorState } from "prosemirror-state"
    import { createRichTextEditor } from './editor';
    import type { LoroText } from 'loro-crdt';
  
    
    const dispatch = createEventDispatcher()
  
    /** @type string */
    export let className = "ui-editor"

    /** @type Loro */
    export let state;
    /** @type string */
    export let document;
  
    /** @type EditorState */
    export let editorState = createRichTextEditor(document, state)
  
    /** @type string */
    export let placeholder = ''
  
    /** Reference to the editor view
     *  @type EditorView|null */
    export let view: EditorView | null = null
  
    /** Debounce change events (set to zero for immediate updates) */
    export let debounceChangeEventsInterval = 50
  
    /** Reference to the editor component
     *  @type HTMLDivElement | null */
    export let editor: HTMLDivElement | null = null
  
    /** Initial EditorView props */
    export let editorViewProps = {}
  
    /** Focus the content-editable div */
    export function focus() {
      view && view.focus()
    }
  
    /** Blur the content-editable div */
    export function blur() {
      editor && editor.blur()
    }
  
    /**
     * Tracks the timeout id of the last time the change event was dispatched
     * @type {number | undefined}
     */
    let dispatchLastEditTimeout: number | undefined
  
    /** Tracks whether changes to editor state were not yet dispatched */
    let isDirty = false
  
   // @ts-ignore
     $: if (view && editorState && !isDirty) {
      view.updateState(editorState) // necessary to keep the DOM in sync with the editor state on external updates
    }
  
    /**
     * Tracks whether the editor is empty (i.e. has a content size of 0)
     * @type {boolean}
     */
    let editorIsEmpty: boolean
    // @ts-ignore
    $: editorIsEmpty = editorState ? editorState.doc.content.size === 0
      || (editorState.doc.textContent === "" && editorState.doc.content.size < 3) : true
  
    /** Dispatches a change event and resets whether the editor state is dirty */
    const dispatchChangeEvent = () => {
      if (isDirty) {
        dispatch('change', {editorState})
        isDirty = false
      }
    }
  
    /**
     * Captures custom events from plugins and dispatches them with a new event type (based on event.detail.type)
     * @param event {CustomEvent}
     */
    const onCustomEvent = (event: CustomEvent) => {
      if (event.detail) {
        const {type, ...detail} = event.detail
        dispatch(type || 'custom', detail)
      }
    }
  
    onMount(() => {
      // @ts-ignore
      view = new EditorView({mount: editor}, {
          ...editorViewProps,
        state: editorState,
        dispatchTransaction: (transaction) => {
          // @ts-ignore
          editorState = view.state.apply(transaction)
  
          // @ts-ignore
          const contentHasChanged = !editorState.doc.eq(view.state.doc)
  
          if (contentHasChanged) {
            isDirty = true
            if (debounceChangeEventsInterval > 0) {
              if (dispatchLastEditTimeout) clearTimeout(dispatchLastEditTimeout)
              dispatchLastEditTimeout = setTimeout(dispatchChangeEvent, 50)
            } else {
              setTimeout(dispatchChangeEvent, 0)
            }
          }
  
          // @ts-ignore
          view.updateState(editorState)
  
          dispatch('transaction', {view, editorState, isDirty, contentHasChanged})
        },
      })
    })
  
    onDestroy(() => {
      // @ts-ignore
      if (view)
        view.destroy()
    })
  
  </script> -->
  <!-- svelte-ignore a11y-no-static-element-interactions -->
  <!-- @ts-ignore -->
  <!-- <div class={className}
       class:ProseMirror={true}
       class:editor_empty={editorIsEmpty}
       data-placeholder={placeholder}
       bind:this={editor}
       on:focus
       on:blur
       on:keydown
  ></div> -->
  
  <style>
  
    :global(body) {
      --ui-color-placeholder: #AAAAAA;
    }
  
    :global(.ProseMirror) {
      position: relative;
    }
  
    :global(.ProseMirror) {
      word-wrap: break-word;
      white-space: pre-wrap;
      -webkit-font-variant-ligatures: none;
      font-variant-ligatures: none;
    }
  
    :global(.ProseMirror) pre {
      white-space: pre-wrap;
    }
  
    :global(.ProseMirror) li {
      position: relative;
    }
  
    :global(.ProseMirror-hideselection *::selection) {
      background: transparent;
    }
  
    :global(.ProseMirror-hideselection *::-moz-selection) {
      background: transparent;
    }
  
    :global(.ProseMirror-hideselection) {
      caret-color: transparent;
    }
  
    :global(.ProseMirror-selectednode) {
      outline: 2px solid #8cf;
    }
  
    /* Make sure li selections wrap around markers */
  
    :global(li.ProseMirror-selectednode) {
      outline: none;
    }
  
    :global(li.ProseMirror-selectednode:after) {
      content: "";
      position: absolute;
      left: -32px;
      right: -2px;
      top: -2px;
      bottom: -2px;
      border: 2px solid #8cf;
      pointer-events: none;
    }
  
    :global(.ProseMirror .empty-node::before) {
      position: absolute;
      color: #aaa;
      cursor: text;
    }
  
    :global(.ProseMirror .empty-node:hover::before) {
      color: #777;
    }
  
    :global(.ProseMirror.editor_empty::before) {
      position: absolute;
      content: attr(data-placeholder);
      pointer-events: none;
      color: var(--ui-color-placeholder);
    }
  
  </style>