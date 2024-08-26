'use strict';

import trim from "trim";


export default blockquote;

var C_NEWLINE = '\n';
var C_TAB = '\t';
var C_SPACE = ' ';
var C_GT = '>';
// TODO:
// - Grow/shrink support
// - Customise AST output

/* Tokenise a blockquote. */
function blockquote(eat: { (arg0: string): any; (arg0: string): any; now: any; }, value: string, silent: boolean) {
    var self = this;
    var offsets = self.offset;
    var tokenizers = self.blockTokenizers;
    var interruptors = [
        ['indentedCode', { commonmark: true }],
        ['fencedCode', { commonmark: true }],
        ['atxHeading', { commonmark: true }],
        ['setextHeading', { commonmark: true }],
        ['thematicBreak', { commonmark: true }],
        ['html', { commonmark: true }],
        ['list', { commonmark: true }],
        ['definition', { commonmark: false }],
        ['footnote', { commonmark: false }]
    ];
    var now = eat.now();
    var currentLine = now.line;
    var length = value.length;
    var values = [];
    var contents = [];
    var indents = [];
    var add;
    var index = 0;
    var character;
    var rest;
    var nextIndex;
    var content;
    var line;
    var startIndex;
    var prefixed;
    var exit;

    while (index < length) {
        character = value.charAt(index);

        if (character !== C_SPACE && character !== C_TAB) {
            break;
        }

        index++;
    }

    if (value.charAt(index) !== C_GT) {
        return;
    }

    const regex = /^>\s*\[!(?<keyword>(.*?))\][\t\f ]?(?<title>.*?)$/i;
    nextIndex = value.indexOf(C_NEWLINE, index);
    content = value.slice(index, nextIndex);
    const m = regex.exec(content); // value.slice(index)
    if (!m) {
        return;
    }
    if (!m.groups?.keyword) return;

    if (silent) {
        return true;
    }

    index = 0;
    let titleLine = true
    while (index < length) {
        nextIndex = value.indexOf(C_NEWLINE, index);
        startIndex = index;
        prefixed = false;

        if (nextIndex === -1) {
            nextIndex = length;
        }

        while (index < length) {
            character = value.charAt(index);

            if (character !== C_SPACE && character !== C_TAB) {
                break;
            }

            index++;
        }

        if (value.charAt(index) === C_GT) {
            index++;
            prefixed = true;

            if (value.charAt(index) === C_SPACE) {
                index++;
            }
        } else {
            index = startIndex;
        }

        //   regex.lastIndex = index

        content = value.slice(index, nextIndex);

        if (!prefixed && !trim(content)) {
            index = startIndex;
            break;
        }

        if (!prefixed) {
            rest = value.slice(index);

            /* Check if the following code contains a possible
             * block. */
            if (interrupt(interruptors, tokenizers, self, [eat, rest, true])) {
                break;
            }
        }

        line = startIndex === index ? content : value.slice(startIndex, nextIndex);

        indents.push(index - startIndex);
        values.push(line);
        if (titleLine) {
            titleLine = false
        } else {
            contents.push(content);
        }

        index = nextIndex + 1;
    }

    index = -1;
    length = indents.length;
    add = eat(values.join(C_NEWLINE));

    while (++index < length) {
        offsets[currentLine] = (offsets[currentLine] || 0) + indents[index];
        currentLine++;
    }

    exit = self.enterBlock();
    const title = self.tokenizeInline(m.groups?.title, now);
    contents = self.tokenizeBlock(contents.join(C_NEWLINE), now);
    exit();
    // console.log(title,)
    return add({
        type: 'callout',
        children: [{
            type: "heading",
            children: title,
            data: { hName: 'svelte:fragment',
            hProperties: {
                slot: "title"
            } }
        }, {
            type: "block",
            children: contents,
            data: { hName: 'svelte:fragment',
            hProperties: {
                slot: "body"
            } }
        }],
        keyword: m.groups?.keyword,
        data: {
            hName: 'Components.Callout',
            hProperties: {
                "calloutType": m.groups?.keyword
            },
        }
    });
    // return add({
    //     type: 'callout',
    //     children: [{
    //         type: "heading",
    //         children: title,
    //         data: { hName: 'div',
    //         hProperties: {
    //             className: "callout-title"
    //         } }
    //     }, {
    //         type: "block",
    //         children: contents,
    //         data: { hName: 'div',
    //         hProperties: {
    //             className: "callout-content"
    //         } }
    //     }],
    //     keyword: m.groups?.keyword,
    //     data: {
    //         hName: 'div',
    //         hProperties: {
    //             className: "callout",
    //             "data-callout": m.groups?.keyword
    //         },
    //     }
    // });
}

function interrupt(interruptors, tokenizers, ctx, params) {
    var bools = ['pedantic', 'commonmark'];
    var count = bools.length;
    var length = interruptors.length;
    var index = -1;
    var interruptor;
    var config;
    var fn;
    var offset;
    var bool;
    var ignore;

    while (++index < length) {
        interruptor = interruptors[index];
        config = interruptor[1] || {};
        fn = interruptor[0];
        offset = -1;
        ignore = false;

        while (++offset < count) {
            bool = bools[offset];

            if (config[bool] !== undefined && config[bool] !== ctx.options[bool]) {
                ignore = true;
                break;
            }
        }

        if (ignore) {
            continue;
        }

        if (tokenizers[fn].apply(ctx, params)) {
            return true;
        }
    }

    return false;
}
