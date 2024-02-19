import {
    SITE_DEFAULT_DESCRIPTION,
    SITE_TITLE,
    SITE_URL,
    RSS_DEFAULT_POSTS_PER_PAGE
} from '$lib/metadata';

import { create } from 'xmlbuilder2';
//   import { base } from '$app/paths';

export const prerender = true;

export async function GET() {
    const headers = {
        'Cache-Control': 'max-age=0, s-maxage=3600',
        'Content-Type': 'application/xml'
    };
    return new Response(await getRssXml(), { headers });
}

const AUTHOR = "Jade Ellis"
// prettier-ignore
async function getRssXml(): Promise<string> {
    const rssUrl = `${SITE_URL}/rss.xml`;
    const root = create({ version: '1.0', encoding: 'utf-8' })
        .ele('feed', {
            xmlns: 'http://www.w3.org/2005/Atom',
        })
        .ele('title').txt(SITE_TITLE).up()
        .ele('link', { href: SITE_URL }).up()
        .ele('link', { rel: 'self', href: rssUrl }).up()
        .ele('updated').txt(new Date().toISOString()).up()
        .ele('id').txt(SITE_URL).up()
        .ele('author')
        .ele('name').txt(AUTHOR).up()
        .up()
        .ele('subtitle').txt(SITE_DEFAULT_DESCRIPTION).up()

    // for await (const post of posts) {
    //     const pubDate = 
    //     const postUrl = 
    //     const postHtml = 
    //     const summary = post.metadata.description;

    //     root.ele('entry')
    //         .ele('title').txt(post.metadata.title).up()
    //         .ele('link', { href: postUrl }).up()
    //         .ele('updated').txt(pubDate).up()
    //         .ele('id').txt(postUrl).up()
    //         .ele('content', { type: 'html' }).txt(postHtml).up()
    //         .ele('summary').txt(summary).up()
    //         .up();
    // }

    return root.end()
}
