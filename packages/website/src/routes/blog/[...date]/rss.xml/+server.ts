import { pages } from '../../posts'

import {
    SITE_DEFAULT_DESCRIPTION,
    SITE_TITLE,
    SITE_URL,
    RSS_DEFAULT_POSTS_PER_PAGE
} from '$lib/metadata';
import rssStyle from "./rss-style.xsl?url"
import rssStyleCss from "./styles.css?url"
import { create } from 'xmlbuilder2';
import { error } from '@sveltejs/kit'
//   import { base } from '$app/paths';

export const prerender = true;

export async function GET({ url, params }) {
    let dateParts = params.date.split(/[\/-]/).filter((s)=>s.length !== 0).map((p) => parseInt(p, 10))
    if (dateParts.length > 3) {
        throw error(404, 'Feed not found (bad date)')
    }

    const selectedPages =  dateParts.length ? pages
        .filter((post) => {
            console.log("filtering")
        let date = new Date(post.date)
        return (
            (!dateParts[0] || date.getFullYear() == dateParts[0]) &&
            (!dateParts[1] || date.getMonth()+1 == dateParts[1]) &&
            (!dateParts[2] || date.getDate() == dateParts[2])
        )
    }) : pages;
    const headers = {
        'Cache-Control': 'max-age=0, s-maxage=3600',
        'Content-Type': 'application/xml'
    };
    url.search = "";
    return new Response(await getRssXml(url.href, selectedPages), { headers });
}

const AUTHOR = "Jade Ellis"
// prettier-ignore
async function getRssXml(selfUrl: string, pages: any[]): Promise<string> {
    // const rssUrl = `${SITE_URL}/rss.xml`;
    const root = create({ version: '1.0', encoding: 'utf-8' })
        .ins('xml-stylesheet', `type="text/xsl" href="${rssStyle}"`)
        .ele('feed', {
            xmlns: 'http://www.w3.org/2005/Atom',
            "xmlns:jade": 'http://jade.ellis.link',
        })
        .ele('jade:link', { rel:"stylesheet", href: rssStyleCss }).up()
        .ele('title').txt(SITE_TITLE).up()
        .ele('link', { href: SITE_URL }).up()
        .ele('link', { rel: 'self', href: selfUrl }).up()
        .ele('updated').txt(new Date().toISOString()).up()
        .ele('id').txt(SITE_URL).up()
        .ele('author')
        .ele('name').txt(AUTHOR).up()
        .up()
        .ele('subtitle').txt(SITE_DEFAULT_DESCRIPTION).up()

    for await (const post of pages) {
        const title = post.title;
        const pubDate = post.date
        const postUrl = SITE_URL + "/blog/" + post.canonical
        // const postHtml = 
        const summary = post.description;

        root.ele('entry')
            .ele('title').txt(title).up()
            .ele('link', { href: postUrl }).up()
            .ele('published').txt(pubDate).up()
            .ele('id').txt(postUrl).up()
            // .ele('content', { type: 'html' }).txt(postHtml).up()
            .ele('summary').txt(summary).up()
            .up();
    }

    return root.end()
}
