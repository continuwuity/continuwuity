// https://github.com/String10/Hakuba/blob/master/package.json
// import { defineMDSveXConfig as defineConfig } from "mdsvex";
// import type { Plugin, Settings } from 'unified';

import remarkGfm from "remark-gfm";
import remarkFrontmatter from "remark-frontmatter";
import remarkWikiLink from "remark-wiki-link";

import remarkMath from "remark-math"
// @ts-ignore
import remarkAbbr from "remark-abbr"
import remarkFootnotes from 'remark-footnotes'
import remarkCallouts from "remark-callouts";

import rehypeKatexSvelte from 'rehype-katex-svelte';
// import github from "remark-github";

import rehypeSlug from 'rehype-slug';
import remarkReadingTime from "remark-reading-time";
// import rehypeToc from '@jsdevtools/rehype-toc';
import { createHighlighter } from "@bitmachina/highlighter";

import { parse, format } from "node:path";

import slugify from 'slugify';

export const NOTE_ICON = '<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="2" x2="22" y2="6"></line><path d="M7.5 20.5 19 9l-4-4L3.5 16.5 2 22z"></path></svg>';

export const QUOTE_ICON = '<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 21c3 0 7-1 7-8V5c0-1.25-.756-2.017-2-2H4c-1.25 0-2 .75-2 1.972V11c0 1.25.75 2 2 2 1 0 1 0 1 1v1c0 1-1 2-2 2s-1 .008-1 1.031V20c0 1 0 1 1 1z"></path><path d="M15 21c3 0 7-1 7-8V5c0-1.25-.757-2.017-2-2h-4c-1.25 0-2 .75-2 1.972V11c0 1.25.75 2 2 2h.75c0 2.25.25 4-2.75 4v3c0 1 0 1 1 1z"></path></svg>';

export const INFO_ICON = '<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"></circle><line x1="12" y1="16" x2="12" y2="12"></line><line x1="12" y1="8" x2="12.01" y2="8"></line></svg>';

export const ICONS = {
    note: NOTE_ICON,
    quote: QUOTE_ICON,
    info: INFO_ICON,
};

import { globSync } from 'glob'
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const projects = globSync('node_modules/Notes/Projects/*.md')
    .map((filepath) => {
        return parse(filepath)
    })
    .map((path) => {
        return format({
            // ...path,
            name: slugify(path.name, { lower: true }),
            // base: undefined,
            // root: "",
            // ext: undefined,
            // dir: path.dir.replace("/node_modules/Notes/Projects", "")
        })
    })
/**
 * @type {string[]}
 */
const permalinks = projects.map((p) => "/projects/" + p)

// console.log(permalinks)

/**
 * @param {string} pageName
 * @returns {string[]}
 */
function pageResolver(pageName) {
    const slug = slugify(pageName, { lower: true });
    return ["/", "/projects/"].map((p) => p + slug);
}
import { grammars } from 'tm-grammars'
// console.log()

/**
 * @param {string} name
 */
function getGrammar(name) {
    const metadata = grammars.find((grammar) => grammar.name == name)
    if (!metadata) {
        throw "Grammar not found"
    }
    return {
        ...metadata,
        id: name,

        grammar: JSON.parse(readFileSync(fileURLToPath(import.meta.resolve('tm-grammars/grammars/' + name + '.json')), 'utf8')),
    }
}

const hrefTemplate = (/** @type {string} */ permalink) => permalink

// function customizeTOC(toc) {
//     // console.log(toc)

//     return {
//         type: 'root',
//         children: [{
//             type: "element",
//             // tagName: "svelte:component",
//             // properties: { this: "{tocComponent}" },
//             tagName: "div",
//             properties: {},
//             children: [toc],
//         }]
//     };
// }
/**
 * @param {{level: number, title: string}[]} headings
 */
function buildNestedHeadings(headings) {
    /**
     * @type {{level: number, title: string, children: unknown}[]}
     */
    const result = [];
    const stack = [{ level: 0, children: result }];

    for (const heading of headings) {
        while (
            stack.length > 1 &&
            heading.level <= stack[stack.length - 1].level
        ) {
            stack.pop();
        }
        const parent = stack[stack.length - 1];
        const newHeading = {
            ...heading,
            children: [],
            level: heading.level,
        };
        parent.children.push(newHeading);
        stack.push(newHeading);
    }

    return result;
}
import { visit } from 'unist-util-visit';
import { toString as mdast_tree_to_string } from 'mdast-util-to-string'


import GithubSlugger from 'github-slugger'
/**
 * @param {{ prefix?: string; }} opts
 */
function add_toc_remark(opts) {
    const slugs = new GithubSlugger()
    const prefix = opts?.prefix || "";
    return async function transformer(tree, vFile) {
        slugs.reset()

        vFile.data.flattenedHeadings = [];

        visit(tree, 'heading', (node) => {
            const title = mdast_tree_to_string(node);
            vFile.data.flattenedHeadings.push({
                level: node.depth,
                title,
                id: prefix + slugs.slug(title)
            });
        });

        if (!vFile.data.fm) vFile.data.fm = {};
        vFile.data.fm.flattenedHeadings = vFile.data.flattenedHeadings;
        vFile.data.fm.headings = buildNestedHeadings(vFile.data.flattenedHeadings);
    };
}

function add_data_to_fm(_opts) {
    return async function transformer(tree, vFile) {
        if (!vFile.data.fm) vFile.data.fm = {};

        vFile.data.fm.readingTime = vFile.data.readingTime;
    };
}
import { toString as hast_tree_to_string } from 'hast-util-to-string'
/**
 * Determines whether the given node is an HTML element.
 */
function isHtmlElementNode(node) {
    return typeof node === "object" &&
        node.type === "element" &&
        typeof node.tagName === "string" &&
        "properties" in node &&
        typeof node.properties === "object";
}
const HEADINGS = ["h1", "h2", "h3", "h4", "h5", "h6"]
/**
 * Determines whether the given node is an HTML heading node, according to the specified options
 */
function isHeadingNode(node) {
    return isHtmlElementNode(node) && HEADINGS.includes(node.tagName);
}
function add_toc_rehype(self, opts) {
    return async function transformer(tree, vFile) {
        // console.log(tree)
        vFile.data.headings = [];

        visit(tree, isHeadingNode, (node) => {
            // console.log(node)
            vFile.data.headings.push({
                level: node.depth,
                title: hast_tree_to_string(node),
            });
        });

        if (!vFile.data.fm) vFile.data.fm = {};
        vFile.data.fm.headings = vFile.data.headings;
    };
}


import toCamel from "just-camel-case";
const RE_SCRIPT_START =
    /<script(?:\s+?[a-zA-z]+(=(?:["']){0,1}[a-zA-Z0-9]+(?<!module)(?:["']){0,1}){0,1})*\s*?>/;
function vite_images_rehype(opts) {
    return async function transformer(tree, vFile) {
        const urls = new Map();
        const url_count = new Map();

        /**
         * @param {string} url
         */
        function transformUrl(url) {
            // url = decodeURIComponent(url)
            // console.log("decoded", url)

            // filenames can start with digits,
            // prepend underscore to guarantee valid module name
            let camel = `_${toCamel(url)}`;
            const count = url_count.get(camel);
            const dupe = urls.get(url);

            if (count && !dupe) {
                url_count.set(camel, count + 1);
                camel = `${camel}_${count}`;
            } else if (!dupe) {
                url_count.set(camel, 1);
            }

            urls.set(url, {
                path: url,
                id: camel
            });

            return camel;


        }
        // console.log(tree)
        // vFile.data.headings = [];

        // console.log(tree)
        visit(tree, { tagName: "img" }, (node) => {
            let url = node.properties.src;
            url = (url.includes("?") ? url + "&" : url + "?") + "url";

            node.properties.src = `{${transformUrl(url)}}`
            // new URL('./img.png', import.meta.url).href
            // vFile.data.headings.push({
            //     level: node.depth,
            //     title: hast_tree_to_string(node),
            // });
        });
        visit(tree, { tagName: "Components.img" }, (node) => {
            let url = node.properties.src;
            const thumb = (url.includes("?") ? url + "&" : url + "?") + "thumb";
            url = (url.includes("?") ? url + "&" : url + "?") + "url";

            node.properties.src = `{${transformUrl(url)}}`
            node.properties.thumb = `{${transformUrl(thumb)}}`
            // node.properties.src = `{new URL('${url}', import.meta.url)}`
            // new URL('./img.png', import.meta.url).href
            // vFile.data.headings.push({
            //     level: node.depth,
            //     title: hast_tree_to_string(node),
            // });
        });

        let scripts = "";
        urls.forEach((x) => (scripts += `import ${x.id} from "./${x.path}";\n`));
        // urls.forEach((x) => (scripts += `const ${x.id} = new URL("${x.path}", import.meta.url);\n`));
        // console.log(scripts)
        // urls.forEach((x) => {
        //     if (x.meta) {
        //         let a = ["src", "width", "height"]
        //         scripts += `import {${a.map((a) => a + " as " + x.id + "_" + a).join(",")}} from "${x.path.includes("?") ? x.path + "&as=metadata" : x.path + "?as=metadata:src;width;height"}";\n`
        //     }
        // });

        let is_script = false;

        visit(tree, { type: "raw" }, (node) => {
            // console.log(node)
            if (RE_SCRIPT_START.test(node.value)) {
                // console.log("inserting")
                is_script = true;
                node.value = node.value.replace(RE_SCRIPT_START, (script) => {
                    return `${script}\n${scripts}`;
                });
            }
        });

        if (!is_script) {
            tree.children.push({
                type: 'raw',
                value: `<script>\n${scripts}</script>`,
            })
        }

    };
}
/**
 * @type {import("mdsvex").MdsvexOptions}
 */
const config = {
    extensions: [".svelte.md", ".md", ".svx"],

    //   fences: true,
    //   ruleSpaces: false,
    smartypants: {
        dashes: "oldschool",
    },

    layout: {
        _: "./src/lib/mdlayouts/default.svelte"
    },

    highlight: {
        // @ts-ignore
        highlighter: await createHighlighter({ theme: "github-dark", langs: ["http", "jsx", "javascript", "typescript", "rust"].map(getGrammar) }),
        alias: {
            ts: "typescript",
            mdx: "markdown",
            svelte: "svelte",
            svx: "svx",
            mdsvex: "svx",
            sig: "ts",
        }
    },

    remarkPlugins: [
        // remarkFrontmatter,
        // [github, {repository}],
        remarkMath,
        remarkAbbr,
        [remarkFootnotes, { inlineNotes: true }],
        remarkGfm,
        [remarkCallouts, {}],
        [remarkWikiLink, {
            // @ts-ignore
            aliasDivider: "|",
            permalinks,
            pageResolver,
            hrefTemplate,

            // wikiLinkClassName,
            // newClassName,
        }],
        // [citePlugin, {
        //   syntax: {
        //     // see micromark-extension-cite
        //     enableAltSyntax: false,
        //     enablePandocSyntax: true,
        //   },
        //   toMarkdown: {
        //     // see mdast-util-cite
        //     standardizeAltSyntax: false,
        //     enableAuthorSuppression: true,
        //     useNodeValue: false,
        //   },
        // }],
        // [remarkBibliography, { bibliography }],
        // [remarkMermaid, {}]
        remarkReadingTime,
        add_data_to_fm,
        [add_toc_remark, { prefix: "h-" }]
    ],
    rehypePlugins: [
        // @ts-ignore
        rehypeKatexSvelte,
        // @ts-ignore
        [rehypeSlug, { prefix: "h-" }],
        vite_images_rehype
    ],
};

export default config;
