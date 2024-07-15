import { browser } from '$app/environment'
// import { format } from 'date-fns'

import slugify from 'slugify';
import { parse, format, relative } from "node:path";

// we require some server-side APIs to parse all metadata
if (browser) {
    throw new Error(`posts can only be imported server-side`)
}
// import { browser } from '$app/environment'
// import { format } from 'date-fns'
// import { parse } from 'node-html-parser'
// import readingTime from 'reading-time/lib/reading-time.js'

// // we require some server-side APIs to parse all metadata
// if (browser) {
//   throw new Error(`posts can only be imported server-side`)
// }

// // Get all posts and add metadata
// export const posts = Object.entries(import.meta.glob('/posts/**/*.md', { eager: true }))
//   .map(([filepath, post]) => {
//     const html = parse(post.default.render().html)
//     const preview = post.metadata.preview ? parse(post.metadata.preview) : html.querySelector('p')

//     return {
//       ...post.metadata,

//       // generate the slug from the file path
//       slug: filepath
//         .replace(/(\/index)?\.md/, '')
//         .split('/')
//         .pop(),

//       // whether or not this file is `my-post.md` or `my-post/index.md`
//       // (needed to do correct dynamic import in posts/[slug].svelte)
//       isIndexFile: filepath.endsWith('/index.md'),

//       // format date as yyyy-MM-dd
//       date: post.metadata.date
//         ? format(
//             // offset by timezone so that the date is correct
//             addTimezoneOffset(new Date(post.metadata.date)),
//             'yyyy-MM-dd'
//           )
//         : undefined,

//       preview: {
//         html: preview.toString(),
//         // text-only preview (i.e no html elements), used for SEO
//         text: preview.structuredText ?? preview.toString()
//       },

//       // get estimated reading time for the post
//       readingTime: readingTime(html.structuredText).text
//     }
//   })
//   // sort by date
//   .sort((a, b) => new Date(b.date).getTime() - new Date(a.date).getTime())
//   // add references to the next/previous post
//   .map((post, index, allPosts) => ({
//     ...post,
//     next: allPosts[index - 1],
//     previous: allPosts[index + 1]
//   }))
const dateRegex = /^((?<year>\d{4})-(?<month>[0][1-9]|1[0-2])-(?<day>[0][1-9]|[1-2]\d|3[01]))\s*/
export const pages = Object.entries(import.meta.glob('/node_modules/Notes/Blogs/*.md', { eager: true }))
    .map(([filepath, post]) => {
        let path = parse(filepath);
        let title = path.name.replace(dateRegex, "")
        
        // @ts-ignore
        // let {year, month, day}: { year: string, month: string, day: string } = path.name.match(dateRegex)?.groups;

        // console.log(year, month, day)
        let date = path.name.match(dateRegex)[1];
        let datePath = date.replaceAll("-", "/")
        let slug = slugify(title, { lower: true })
        return {
            title,
            date,
            canonical: datePath + "/" + slug,
            // @ts-ignore
            ...post.metadata,

            slug,
            // filepath: relative(import.meta.dirname, filepath)
            filepath: path
        }
    })
    // sort by date
    .sort((a, b) => new Date(b.date).getTime() - new Date(a.date).getTime())
// Get all posts and add metadata
// // add references to the next/previous post
// .map((post, index, allPosts) => ({
//   ...post,
//   next: allPosts[index - 1],
//   previous: allPosts[index + 1]
// }))


// function addTimezoneOffset(date) {
//   const offsetInMilliseconds = new Date().getTimezoneOffset() * 60 * 1000
//   return new Date(new Date(date).getTime() + offsetInMilliseconds)
// }