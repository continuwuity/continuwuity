// https://github.com/String10/Hakuba/blob/master/package.json
// import { defineMDSveXConfig as defineConfig } from "mdsvex";
// import type { Plugin, Settings } from 'unified';

import remarkGfm from "remark-gfm";
import remarkFrontmatter from "remark-frontmatter";
import remarkWikiLink, { } from "remark-wiki-link";

import remarkMath from "remark-math"
// @ts-ignore
import remarkAbbr from "remark-abbr"
import remarkFootnotes from 'remark-footnotes'

import rehypeKatexSvelte from 'rehype-katex-svelte';
// import github from "remark-github";

import rehypeSlug from 'rehype-slug';

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

const projects = globSync('/node_modules/Notes/Projects/*.md')
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



/**
 * @param {string} pageName
 * @returns {string[]}
 */
function pageResolver(pageName) {
    const slug = slugify(pageName, { lower: true });
    return ["/", "/projects/"].map((p) => p + slug);
}

const hrefTemplate = (/** @type {string} */ permalink) => `#${permalink}`
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

    highlight: {
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
        [remarkWikiLink, {
            // @ts-ignore
            aliasDivider: "|",
            permalinks: permalinks,
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
    ],
    rehypePlugins: [
        // @ts-ignore
        rehypeKatexSvelte,
        // @ts-ignore
        rehypeSlug
    ],
};

export default config;
