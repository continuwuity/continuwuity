<script lang="ts">
    import { page } from "$app/stores";

    export let data;
    let { pages } = data;
    // $: console.log(data);
</script>

<section role="feed">
    <h1>Blog Posts</h1>
    {#each pages as post, index}
        <article aria-posinset={index + 1} aria-setsize={pages.length}>
            <div class="content" data-sveltekit-preload-data="hover">
                <h2>
                    <a href="/blog/{post.canonical}" class="text-link">
                        {post.title}
                    </a>
                </h2>
                <span class="quiet">{post.date}</span>
                {#if post.description}
                <p>{post.description}</p>
                {/if}
            </div>
        </article>
    {:else}
        <p>No posts yet!</p>
    {/each}
    <!-- {#if showPosts < postCount}
		<button type="submit" on:click={handleClick}>See more {H_ELLIPSIS_ENTITY}</button>
	{/if} -->
</section>
