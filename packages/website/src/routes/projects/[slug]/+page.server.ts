import { pages } from './projects'
import { error } from '@sveltejs/kit'
import TTLCache, { } from "@isaacs/ttlcache";
import { parse } from "@tusbar/cache-control";
const cache = new TTLCache({ max: 10000, ttl: 1000 })

import type { Endpoints } from "@octokit/types";

let repoRegex = new RegExp("https?://github\.com/(?<repo>[a-zA-Z0-9]+/[a-zA-Z0-9]+)/?")

/** @type {import('./$types').PageServerLoad} */
export async function load({ params }) {
    const { slug } = params

    // get post with metadata
    const page = pages.find((post) => slug === post.slug)

    if (!page) {
        throw error(404, 'Post not found')
    }

    let ghReleaseData: Endpoints["GET /repos/{owner}/{repo}/releases/latest"]["response"]["data"] | undefined;

    let repo = (page.repo as string).match(repoRegex)?.groups?.repo
    if (repo) {
        if (!cache.has(repo)) {
            // console.log("cache miss")
            ghReleaseData = await fetch("https://api.github.com/repos/" + repo + "/releases/latest").then(async (res) => {
                let json = await res.json()
                let ttl = (parse(res.headers.get("cache-control") || undefined)?.maxAge || 60) * 1000
                cache.set(repo, json, { ttl })
                return json
            })
        } else {
            // console.log("cache hit")
            ghReleaseData = cache.get(repo) as typeof ghReleaseData
        }
        // .then((data) => {
        //     // console.log(data)
        // })

    }

    return {
        page,
        ghReleaseData
    }
}