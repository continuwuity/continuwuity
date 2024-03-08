// https://github.com/jasongitmail/super-sitemap/
import * as sitemap from 'super-sitemap';
import { SITE_URL } from '$lib/metadata';
import type { RequestHandler } from '@sveltejs/kit';

export const GET: RequestHandler = async ({ params }) => {
    return await sitemap.response({
        origin: SITE_URL,
        page: params.page,
    });
};