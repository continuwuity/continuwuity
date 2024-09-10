<script lang="ts">
    import { page } from "$app/stores";
    import * as Sentry from "@sentry/sveltekit";
    import { onMount } from "svelte";
    import SvelteSeo from "svelte-seo";

    const { status, error } = $page;
    const message = error?.message || "Hmm";
    const title = `${status}: ${message}`;
    let sentryElement: HTMLDivElement;
    let openForm = () => {};
    onMount(async () => {
        const feedback = Sentry.getFeedback({
            el: sentryElement,
        });
        if (!feedback) {
            return;
        }
        //     console.log("feedback", feedback);
        const form = await feedback.createForm({});
        form.appendToDom();
        form.open();
        openForm = async () => {
            form.open();
        };
    });
</script>

<SvelteSeo {title} />

<main class="main container" id="page-content">
    <div class="wrapper">
        <h1>{title}</h1>
        <div bind:this={sentryElement} class="feedback"></div>
        <button on:click={openForm}>Send Feedback</button>
        <button class="secondary" on:click={()=> window.location.reload()}>Reload</button>
        <!-- <div role="doc-subtitle">{status}</div> -->
    </div>
</main>

<style>
    main {
        display: grid;
        place-items: center;
    }
    .wrapper {
        flex-grow: 2;
        width: 40vw;
        max-width: 500px;
        margin: 0 auto;
        /* display: flex;
        flex-direction: column;
        align-items: center;
        gap: var(--spacing);
        margin: 48px auto;
        max-width: 320px;
        padding: 3rem 0.5rem; */
    }

    h1 {
        margin: 0;
        font-size: 4em;
        /* font-weight: 100; */
    }
    /* [role="doc-subtitle"] {
        padding-block-start: 0;
        font-size: 2em;
        font-weight: 600;
    } */
    :global(#sentry-feedback) {
        --dialog-inset: auto auto 0;
    }
    p {
        width: 95%;
        font-size: 1.5em;
        line-height: 1.4;
    }
</style>
