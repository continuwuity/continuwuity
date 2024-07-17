import { pages } from '../../posts'

import type Feed from '@json-feed-types/1_1'
import {
    SITE_DEFAULT_DESCRIPTION,
    SITE_TITLE,
    SITE_URL,
    RSS_DEFAULT_POSTS_PER_PAGE
} from '$lib/metadata';

import { error } from '@sveltejs/kit'
//   import { base } from '$app/paths';

export const prerender = true;

export async function GET({ params, url}) {
    let dateParts = params.date.split(/[\/-]/).filter((s)=>s.length !== 0).map((p) => parseInt(p, 10))
    if (dateParts.length > 3) {
        throw error(404, 'Feed not found (bad date)')
    }
    
    const selectedPages =  dateParts.length ? pages
        .filter((post) => {
        let date = new Date(post.date)
        return (
            (!dateParts[0] || date.getFullYear() == dateParts[0]) &&
            (!dateParts[1] || date.getMonth()+1 == dateParts[1]) &&
            (!dateParts[2] || date.getDate() == dateParts[2])
        )
    }) : pages;
    const headers = {
        'Cache-Control': 'max-age=0, s-maxage=3600',
        'Content-Type': 'application/feed+json'
    };
    return new Response(await getJsonFeed(url.href, selectedPages), { headers });
}

const AUTHOR = "Jade Ellis"
// prettier-ignore
async function getJsonFeed(selfUrl: string, pages: any[]): Promise<string> {

    const feed: Feed = {
        version: 'https://jsonfeed.org/version/1.1',
        title: SITE_TITLE,
        icon: `${SITE_URL}/android-chrome-256x256.png`,
        home_page_url: SITE_URL,
        description: SITE_DEFAULT_DESCRIPTION,
        feed_url: selfUrl,
        authors: [{ name: AUTHOR }],
        items: [
        ],
    }
    
    for await (const post of pages) {
        const title = post.title;
        const pubDate = post.date
        const postUrl = SITE_URL + "/blog/" + post.canonical
        // const postHtml = 
        const summary = post.description;
        let item: typeof feed.items[number] = {
            id: post.postUrl,
            title,
            url: postUrl,
            date_published: pubDate,
            summary,
            content_text: "",
        }
        feed.items.push(item)
    }

    return JSON.stringify(feed)
}
