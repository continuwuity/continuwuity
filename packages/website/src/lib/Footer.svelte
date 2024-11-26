<script lang="ts">
    import * as Sentry from "@sentry/sveltekit";
    import url from "./logo.svg?url";
    import { SITE_URL } from "$lib/metadata";

    /** @type {Record<string, { href: string; title: string; }[]>} */
    const links = {
        Connect: [
            {
                href: "https://matrix.to/#/@jade:ellis.link",
                title: "Matrix",
            },
            { href: "https://github.com/JadedBlueEyes", title: "GitHub" },
            { href: "https://tech.lgbt/@JadedBlueEyes", title: "Mastodon" },
            {
                href: "https://bsky.app/profile/jade.ellis.link",
                title: "Bluesky",
            },
            {
                href: "https://www.linkedin.com/in/jadedblueeyes",
                title: "LinkedIn",
            },
        ],
        Feeds: [
            { href: SITE_URL + "/blog/rss.xml", title: "RSS (Atom)" },
            { href: SITE_URL + "/blog/feed.json", title: "JSON Feed" },
        ]
    };
    const sendFeedback = async () => {
        const feedback = Sentry.getFeedback();
        if (!feedback) {
            return;
        }
        const form = await feedback.createForm({});
        form.appendToDom();
        form.open();
    };
</script>

<div class="background">
    <footer class="container">
        <div class="logo">
            <a class="footer-link-home" href={SITE_URL}>
                <img
                    src={url}
                    class="footer-logo"
                    alt=""
                    width="28"
                    height="28"
                />
                <span class="site-name">Jade Ellis</span>
            </a>
            
            <button onclick={sendFeedback} class="feedback-button">Report a bug</button>
        </div>

        {#each Object.entries(links) as [title, inner_links]}
            <div class="links">
                <h2>{title}</h2>
                {#each inner_links as { href, title }}
                    <a {href}>{title}</a>
                {/each}
            </div>
        {/each}

        <div class="copyright">© 2024 Jade Ellis</div>
        <!-- <div class="feedback">
        </div> -->
    </footer>
</div>

<style>
    .background {
        background-color: var(--surface-color);
        margin-block-start: 4em;
    }
    footer {
        padding: 12px var(--spacing);
        margin: 0 auto;
        display: grid;
        grid-template-columns: repeat(2, 1fr);
        grid-template-rows: 1fr;
        grid-row-gap: 6rem;
    }
    .container {
        --container-max-width: calc(var(--page-width) + 6rem + 16px);
    }

    footer h2 {
        font-size: var(--sk-text-m);
        padding-bottom: 1rem;
    }

    .links a {
        display: block;
        line-height: 1.8;
    }

    .copyright {
        grid-column: span 2;
    }

    @media (min-width: 500px) {
        footer {
            grid-template-columns: repeat(3, 1fr);
        }

        footer .logo {
            display: block;
        }

        .copyright {
            grid-column: span 1;
        }

        .feedback {
            display: block;
        }
    }

    .footer-logo {
        width: 3rem;
        height: 100%;
    }

    .footer-link-home {
        display: flex;
        gap: 4px;
        align-items: center;
        padding: 8px;
        font-weight: 700;
    }
    
    .feedback-button {
        margin: 8px;
    }
</style>
