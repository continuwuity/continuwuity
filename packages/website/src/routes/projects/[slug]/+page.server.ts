import { pages } from './projects'
import { error } from '@sveltejs/kit'

/** @type {import('./$types').PageServerLoad} */
export async function load({ params }) {
    const { slug } = params

    // get post with metadata
    const page = pages.find((post) => slug === post.slug)

    if (!page) {
        throw error(404, 'Post not found')
    }

    return {
        page
    }
}