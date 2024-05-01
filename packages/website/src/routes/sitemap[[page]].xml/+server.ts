// https://github.com/jasongitmail/super-sitemap/
import * as sitemap from 'super-sitemap';
import { SITE_URL } from '$lib/metadata';
import type { RequestHandler } from '@sveltejs/kit';

import slugify from 'slugify';

import { parse, format } from "node:path";

const pages = Object.entries(import.meta.glob('/node_modules/Notes/Projects/*.md', { eager: true }))
    .map(([filepath, post]) => {
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

export const GET: RequestHandler = async ({ params }) => {
    return await sitemap.response({
        origin: SITE_URL,
        page: params.page,
        paramValues: {
            '/projects/[slug]': pages
        }
    });
};