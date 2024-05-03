// import { pages } from './projects'
import { error } from '@sveltejs/kit'


/** @type {import('./$types').PageServerLoad} */
export async function load({ data }) {

    // if (!post) {
    //   throw error(404, 'Post not found')
    // }
    // load the markdown file based on slug
    const component =
        // await import(data.page.filepath)
        await import("Notes/Projects/" + data.page.filepath.name + ".md")
    // console.log(data.page.filepath)


    return {
        post: data.page,
        ghReleaseData: data.ghReleaseData,
        component: component.default
    }
}