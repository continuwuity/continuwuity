import { pages } from '../posts'
import { error } from '@sveltejs/kit'

import satori from 'satori';
import { Resvg } from '@resvg/resvg-js';
import { SITE_DOMAIN } from '$lib/metadata';
import TTLCache, { } from "@isaacs/ttlcache";
import { format } from "@tusbar/cache-control";
const cache = new TTLCache({ max: 10000, ttl: 1000 * 60 * 60 })

// import type { Endpoints } from "@octokit/types";

// let repoRegex = new RegExp("https?://github\.com/(?<repo>[a-zA-Z0-9]+/[a-zA-Z0-9]+)/?")



const fontFile = await fetch('https://og-playground.vercel.app/inter-latin-ext-700-normal.woff');
const fontData: ArrayBuffer = await fontFile.arrayBuffer();

// const height = 630;
// const width = 1200;
const defaultWidth = 800;
const defaultRatio = 0.5
// const defaultWidth = 800;

const h = (type: any, props: any) => { return { type, props } }

/** @type {import('./$types').RequestHandler} */
export async function GET({ url }) {
    const slug = url.searchParams.get('slug')
    let dateParts = url.searchParams.get('date')?.split(/[\/-]/)?.map((p: string) => parseInt(p, 10))
    if (dateParts && dateParts.length > 3) {
        throw error(404, 'Post not found (bad date)')
    }
    const width = Number(url.searchParams.get('width'))
    const ratio = Number(url.searchParams.get('ratio'))
    if (width > 10000 || ratio > 50) {
        throw error(400, 'Image too big')
    }
    let image;
    if (!cache.has(slug + "/" + dateParts?.join("-") + "/" + width + "/" + ratio)) {

        // let start = new Date(dateParts[0] || 1, dateParts[1] || 0, dateParts[2] || 0);
        // // @ts-ignore
        // let end = new Date(...dateParts);
        // console.log(dateParts)

        // get post with metadata
        const page = pages
            .filter((post) => slug === post.slug)
            .filter((post) => {
                if (dateParts) {
                    let date = new Date(post.date)
                    return (
                        (!dateParts[0] || date.getFullYear() == dateParts[0]) &&
                        (!dateParts[1] || date.getMonth() + 1 == dateParts[1]) &&
                        (!dateParts[2] || date.getDate() == dateParts[2])
                    )
                } else { return true }
            })[0]

        if (!page) {
            throw error(404, 'Post not found')
        }
        let template = h("div", {
            style: {
                display: 'flex',
                height: '100%',
                width: '100%',
                padding: '10px 20px',
                // alignItems: 'center',
                justifyContent: 'center',
                flexDirection: 'column',
                backgroundImage: 'linear-gradient(to bottom, #dbf4ff, #eff3fc)',
                fontSize: 60,
                // letterSpacing: -2,
                fontWeight: 700
                // textAlign: 'center',
            },
            children: [h("div", {
                style: {
                    fontSize: 15,
                    fontWeight: 600,
                    textTransform: 'uppercase',
                    letterSpacing: 1,
                    margin: '25px 0 10px',
                    color: 'gray'
                },
                children: SITE_DOMAIN
            }), h("div", {
                style: {
                    backgroundImage: 'linear-gradient(90deg, rgb(22, 61, 120), rgb(30, 42, 85))',
                    backgroundClip: 'text',
                    '-webkit-background-clip': 'text',
                    color: 'transparent'
                },
                children: page.title
            }), h("aside", {
                style: {
                    fontSize: 20,
                    fontWeight: 500,
                    color: '#202020',
                    margin: '10px 0 10px'
                },
                children: `Published on ${new Date(page.date).toLocaleDateString()} by Jade Ellis · ${page.readingTime.text}`
            })]
        });
        const svg = await satori(template, {
            fonts: [
                {
                    name: 'Inter Latin',
                    data: fontData,
                    style: 'normal'
                }
            ],
            height: defaultWidth * (ratio || defaultRatio),
            width: defaultWidth,
        });

        const resvg = new Resvg(svg, {
            fitTo: {
                mode: 'width',
                value: width || defaultWidth
            }
        });

        image = resvg.render().asPng();
        cache.set(slug + "/" + dateParts?.join("-") + "/" + width, image)
        ;
    } else {
        image = cache.get(slug + "/" + dateParts?.join("-") + "/" + width) as Buffer
    }
    
    return new Response(image, {
        headers: {
            'Content-Type': 'image/png',
            'Cache-Control': format({
                public: true,
                // immutable: true
                maxAge: 60 * 60 * 24
            }),
            'Cross-Origin-Resource-Policy': 'cross-origin'
        }
    });
}