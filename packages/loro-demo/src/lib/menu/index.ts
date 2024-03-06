import { Plugin, type Command } from "prosemirror-state";
import { setBlockType, toggleMark } from "prosemirror-commands";
import { richTextSchema as schema } from "../prosemirror/schema";

import Menu from "./Menu.svelte";
import type { EditorView } from "prosemirror-view";

class MenuView {
  items: { command: Command; text: string; name: string; }[];
  editorView: EditorView;
  dom: HTMLDivElement;
  menu: Menu;
  constructor(items: { command: Command; text: string; name: string; }[], editorView: EditorView) {
    this.items = items;
    this.editorView = editorView;
    this.update();

    const onCommand = (command: (arg0: any, arg1: any, arg2: any) => void) => {
      editorView.focus();
      command(editorView.state, editorView.dispatch, editorView);
    };

    this.dom = document.createElement("div");
    this.menu = new Menu({
      target: this.dom,
      props: {
        items,
        onCommand
      }
    });
  }

  update() {
    // this.items.forEach(({ command, dom }) => {
    //   let active = command(this.editorView.state, null, this.editorView);
    //   dom.style.display = active ? "" : "none";
    // });
  }

  destroy() {
    this.dom.remove();
  }
}

function menuPlugin(items: { command: Command; text: string; name: string; }[]) {
  return new Plugin({
    view(editorView) {
      let menuView = new MenuView(items, editorView);
      editorView.dom?.parentNode?.insertBefore(menuView.dom, editorView.dom);
      return menuView;
    }
  });
}

let menu = menuPlugin([
  { command: toggleMark(schema.marks.strong), text: "B", name: "strong" },
  { command: toggleMark(schema.marks.em), text: "i", name: "em" },
  { command: toggleMark(schema.marks.code), text: "</", name: "code" },
  { command: setBlockType(schema.nodes.heading), text: "H", name: "heading" },
  { command: setBlockType(schema.nodes.paragraph), text: "p", name: "paragraph" },
  // {
  //   command: setBlockType(schema.nodes.paragraph),
  //   dom: icon("p", "paragraph")
  // },
]);

export default menu;
