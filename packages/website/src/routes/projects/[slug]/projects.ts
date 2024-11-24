import { browser } from '$app/environment'
// import { format } from 'date-fns'

import slugify from 'slugify';
import { parse, format, relative } from "node:path";

// we require some server-side APIs to parse all metadata
if (browser) {
    throw new Error(`posts can only be imported server-side`)
}

export const pages = Object.entries(import.meta.glob('$notes/Projects/*.md', { eager: true }))
    .map(([filepath, post]) => {
        const path = parse(filepath);
        const slug = slugify(path.name, { lower: true })
        return {
            title: path.name,
            // @ts-ignore
            ...post.metadata,
            slug,
            // filepath: relative(import.meta.dirname, filepath)
            filepath: path
        }
    })
// Get all posts and add metadata
// sort by date
// .sort((a, b) => new Date(b.date).getTime() - new Date(a.date).getTime())
// // add references to the next/previous post
// .map((post, index, allPosts) => ({
//   ...post,
//   next: allPosts[index - 1],
//   previous: allPosts[index + 1]
// }))