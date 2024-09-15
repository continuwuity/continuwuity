import { pages } from '../../posts'
import { error } from '@sveltejs/kit'
// import TTLCache, { } from "@isaacs/ttlcache";
// import { parse } from "@tusbar/cache-control";
// const cache = new TTLCache({ max: 10000, ttl: 1000 })

// import type { Endpoints } from "@octokit/types";

// let repoRegex = new RegExp("https?://github\.com/(?<repo>[a-zA-Z0-9]+/[a-zA-Z0-9]+)/?")

/** @type {import('./$types').PageServerLoad} */
export async function load({ params }) {
    const { slug } = params
    const dateParts = params.date.split(/[\/-]/).map((p) => Number.parseInt(p, 10))
    if (dateParts.length > 3) {
        throw error(404, 'Post not found (bad date)')
    }

    // let start = new Date(dateParts[0] || 1, dateParts[1] || 0, dateParts[2] || 0);
    // // @ts-ignore
    // let end = new Date(...dateParts);
    // console.log(dateParts)

    // get post with metadata
    const page = pages
        .filter((post) => slug === post.slug)
        .filter((post) => {
        const date = new Date(post.date)
        return (
            (!dateParts[0] || date.getFullYear() == dateParts[0]) &&
            (!dateParts[1] || date.getMonth()+1 == dateParts[1]) &&
            (!dateParts[2] || date.getDate() == dateParts[2])
        )
    })[0]

    if (!page) {
        throw error(404, 'Post not found')
    }

    return {
        page
    }
}