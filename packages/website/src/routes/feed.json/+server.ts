import type Feed from '@json-feed-types/1_1'
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
        'Content-Type': 'application/feed+json'
    };
    return new Response(await getJsonFeed(), { headers });
}

const AUTHOR = "Jade Ellis"
// prettier-ignore
async function getJsonFeed(): Promise<string> {
    const feedUrl = `${SITE_URL}/feed.json`;

    const feed: Feed = {
        version: 'https://jsonfeed.org/version/1.1',
        title: SITE_TITLE,
        icon: `${SITE_URL}/android-chrome-256x256.png`,
        home_page_url: SITE_URL,
        description: SITE_DEFAULT_DESCRIPTION,
        feed_url: feedUrl,
        authors: [{ name: AUTHOR }],
        items: [
        ],
    }
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

    return JSON.stringify(feed)
}
