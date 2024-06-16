import { type LanguageSupport, indentUnit } from "@codemirror/language";
import { type Extension, EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";

import { indentWithTab } from "@codemirror/commands";
import { basicSetup } from "codemirror";

export function get_base_extensions(
    useTab: boolean,
    tabSize: number,
    lineWrapping: boolean,
    placeholder: string | HTMLElement | null | undefined,
    editable: boolean,
    readonly: boolean,
    lang: LanguageSupport | null | undefined
): Extension[] {
    const extensions: Extension[] = [
        indentUnit.of(" ".repeat(tabSize)),
        EditorView.editable.of(editable),
        EditorState.readOnly.of(readonly),
        basicSetup
    ];

    // @ts-ignore
    if (useTab) extensions.push(keymap.of([indentWithTab]));
    // if (placeholder) extensions.push(placeholderExt(placeholder));
    if (lang) extensions.push(lang);
    if (lineWrapping) extensions.push(EditorView.lineWrapping);

    return extensions;
}