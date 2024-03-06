import {
    wrapIn, setBlockType, chainCommands, toggleMark, exitCode,
    joinUp, joinDown, lift, selectParentNode
} from "prosemirror-commands";
import { wrapInList, splitListItem, liftListItem, sinkListItem } from "prosemirror-schema-list";
import { undo, redo } from "prosemirror-history";
import { undoInputRule } from "prosemirror-inputrules";
import { richTextSchema } from "./schema";
import { keymap } from "prosemirror-keymap"

import type { Command, EditorState, Transaction } from "prosemirror-state";
import type { EditorView } from "prosemirror-view";
import type { Schema } from "prosemirror-model";
const mac = typeof navigator != "undefined" ? /Mac/.test(navigator.platform) : false;
type keyMap = { [x: string]: { combo: string, command: Command }[] };

const addKey = (keyMap: keyMap, name: string, combo: string, command: Command) => {
    if (!keyMap[name]) keyMap[name] = [];
    keyMap[name].push({ combo, command });
}

const createKeyMapConfiguration = (schema: Schema) => {
    let config: keyMap = {}

    addKey(config, 'undo', "Mod-z", undo)

    addKey(config, "redo", "Shift-Mod-z", redo);
    addKey(config, "undoInputRule", "Backspace", undoInputRule);

    if (!mac) addKey(config, "redo", "Mod-y", redo);

    addKey(config, "joinUp", "Alt-ArrowUp", joinUp);
    addKey(config, "joinDown", "Alt-ArrowDown", joinDown);
    addKey(config, "lift", "Mod-BracketLeft", lift);
    addKey(config, "selectParentNode", "Escape", selectParentNode);

    if (!!schema.marks.strong) {
        addKey(config, "toggleMarkStrong", "Mod-b", toggleMark(schema.marks.strong));
        addKey(config, "toggleMarkStrong", "Mod-B", toggleMark(schema.marks.strong));
    }

    if (!!schema.marks.em) {
        addKey(config, "toggleMarkEm", "Mod-i", toggleMark(schema.marks.em));
        addKey(config, "toggleMarkEm", "Mod-I", toggleMark(schema.marks.em));
    }
    if (!!schema.marks.code)
        addKey(config, "toggleMarkCode", "Mod-`", toggleMark(schema.marks.code));

    if (!!schema.nodes.bullet_list)
        addKey(config, "wrapInListUnordered", "Shift-Ctrl-8", wrapInList(schema.nodes.bullet_list));

    if (!!schema.nodes.ordered_list)
        addKey(config, "wrapInListOrdered", "Shift-Ctrl-9", wrapInList(schema.nodes.ordered_list));

    if (!!schema.nodes.blockquote)
        addKey(config, "wrapInBlockquote", "Ctrl->", wrapIn(schema.nodes.blockquote));

    if (!!schema.nodes.hard_break) {
        let br = schema.nodes.hard_break
        const cmd = chainCommands(exitCode, (state, dispatch) => {
            if (dispatch)
                dispatch(state.tr.replaceSelectionWith(br.create()).scrollIntoView())
            return true
        })

        addKey(config, "hardBreak", "Mod-Enter", cmd);
        addKey(config, "hardBreak", "Shift-Enter", cmd);
        if (mac) addKey(config, "hardBreak", "Ctrl-Enter", cmd);
    }

    if (!!schema.nodes.list_item) {
        addKey(config, "splitListItem", "Enter", splitListItem(schema.nodes.list_item));
        addKey(config, "liftListItem", "Mod-[", liftListItem(schema.nodes.list_item));
        addKey(config, "sinkListItem", "Mod-]", sinkListItem(schema.nodes.list_item));
    }
    if (!!schema.nodes.paragraph)
        addKey(config, "setBlockTypeParagraph", "Shift-Ctrl-0", setBlockType(schema.nodes.paragraph));

    if (!!schema.nodes.code_block)
        addKey(config, "setBlockTypeCode", "Shift-Ctrl-\\", setBlockType(schema.nodes.code_block));

    if (!!schema.nodes.heading)
        for (let i = 1; i <= 6; i++) {
            addKey(config, `setHeading${i}`, `Shift-Ctrl-${i}`, setBlockType(schema.nodes.heading, { level: i }));
        }

    if (!!schema.nodes.horizontal_rule) {
        addKey(config, "insertHorizontalRuler", "Mod-_", (state, dispatch) => {
            let hr = schema.nodes.horizontal_rule;
            if (dispatch)
                dispatch(state.tr.replaceSelectionWith(hr.create()).scrollIntoView());
            return true;
        })
    }

    return config
}

const getKeyMapFromConfig = (config: keyMap) => {
    const keys = Object.keys(config);
    let bindings: { [ combo: string]: Command} = {};
    keys.forEach(key => {
        config[key].forEach(entry => {
            bindings[entry.combo] = entry.command;
        })
    })
    return keymap(bindings);
}

const richTextKeyMapConfiguration = createKeyMapConfiguration(richTextSchema);
export const richTextKeyMapPlugin = getKeyMapFromConfig(richTextKeyMapConfiguration);