import { pages } from '../posts'
import { error, type RequestHandler } from '@sveltejs/kit'

import satori from 'satori';
import { Resvg } from '@resvg/resvg-js';
import { SITE_DOMAIN } from '$lib/metadata';
import TTLCache from "@isaacs/ttlcache";
import { format } from "@tusbar/cache-control";
const cache = new TTLCache({ max: 10000, ttl: 1000 * 60 * 60 })
import fnv from "fnv-plus"

const fontFile = await fetch('https://og-playground.vercel.app/inter-latin-ext-700-normal.woff');
const fontData: ArrayBuffer = await fontFile.arrayBuffer();

const defaultWidth = 800;
const defaultRatio = 0.5

// JSX stub
const h = (type: any, props: any) => { return { type, props } }

type a = RequestHandler;
/** @type {RequestHandler} */
export async function GET({ url, request }) {
    // First, get the information about the post
    // We have the slug and date of the post, which we can use to look up the post
    const slug = url.searchParams.get('slug')
    const dateParts = url.searchParams.get('date')?.split(/[\/-]/)?.map((p: string) => Number.parseInt(p, 10))
    if (dateParts && dateParts.length > 3) {
        throw error(404, 'Post not found (bad date)')
    }
    // Next, get the width and ratio of the image
    // to determine the size of the image
    const width = Number(url.searchParams.get('width'))
    const ratio = Number(url.searchParams.get('ratio'))
    // If the width or ratio is too big, don't render the image to prevent DoS attacks
    if (width > 10000 || ratio > 50) {
        throw error(400, 'Image too big')
    }
    let image;

    // Look up the post in the database
    const page = pages
        .filter((post) => slug === post.slug)
        .filter((post) => {
            if (dateParts) {
                const date = new Date(post.date)
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

    // Generate a cache key based on the post's canonical URL, reading time, width, and ratio
    // Caching the image based on this key ensures that the image is not regenerated every time
    // The cache key is also used for browser caching
    const cache_key = fnv.hash(page.canonical + "\x00" + page.readingTime.text + "\x00" + width + "\x00" + ratio).str()

    const received_etag = request.headers.get("if-none-match");
    // If the client has a cached version of the image, return a 304 Not Modified response, indicating that the image has not changed
    // This means we don't even have to have the image cached in memory
    if (received_etag == cache_key) {
        return new Response(null, { status: 304 })
    }

    // If the image is not cached, generate the image and cache it
    if (!cache.has(cache_key)) {
        // First, render the HTML / JSX-based template
        const template = h("div", {
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
                    backgroundImage: 'linear-gradient(90deg, rgb(30, 42, 85), rgb(22, 61, 120))',
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
        // Then, convert the vdom to SVG using satori
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

        // Then, convert the SVG to a PNG image using resvg
        const resvg = new Resvg(svg, {
            fitTo: {
                mode: 'width',
                value: width || defaultWidth
            }
        });

        image = resvg.render().asPng();
        // Finally, save the image to the cache
        cache.set(cache_key, image);
    } else {
        // If the image is cached, return it
        image = cache.get(cache_key) as Buffer
    }
    // Finally, return the image as a response
    return new Response(image, {
        headers: {
            'Content-Type': 'image/png',
            // Cache the image for 24 hours
            'Cache-Control': format({
                public: true,
                // immutable: true
                maxAge: 60 * 60 * 24
            }),
            // Set the cache key as the ETag
            'ETag': cache_key,
            // Allow cross-origin requests to serve the image
            'Cross-Origin-Resource-Policy': 'cross-origin'
        }
    });
}