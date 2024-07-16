<script lang="ts">
    import url from "./logo.svg?url";
    import { SITE_URL } from "$lib/metadata";
    // https://www.aleksandrhovhannisyan.com/
    // https://github.com/AleksandrHovhannisyan/aleksandrhovhannisyan.com/tree/master
</script>

<header id="navbar">
    <div class="container">
        <nav aria-label="Primary">
            <a href="#page-content" class="screen-reader-only skip-navigation"
                >Skip to content</a
            >
            <ul class="navbar-links">
                <li>
                    <a class="navbar-link-home" href={SITE_URL}>
                        <img
                            src={url}
                            class="navbar-logo"
                            alt=""
                            width="28"
                            height="28"
                        />
                        <span class="site-name">Jade Ellis</span>
                    </a>
                </li>
                <!-- <li><a href="/about/" class="navbar-link">About</a></li>
                <li><a href="/art/" class="navbar-link">Art</a></li> -->
                <li><a href="/blog/" class="navbar-link">Blog</a></li>
            </ul>
        </nav>
    </div>
</header>

<style>
    #navbar {
        flex-shrink: 0;
        width: 100%;
        padding-block: 12px;

        background-color: var(--surface-color);
        overflow-x: auto;
        color: #fff;
        
    }
    .container {
        --container-max-width: calc(var(--page-width) + 6rem + 16px);
    }
    #navbar a {
        text-decoration: none;
    }
    #navbar nav {
        white-space: nowrap;
        flex: 1 0 auto;
        position: relative;
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 16px;
    }
    .navbar-logo {
        width: 3rem;
        height: 100%;
    }
    .navbar-links {
        width: 100%;
        display: flex;
        align-items: center;
        gap: 0.25em;
        list-style: none;
        margin: 0;
        padding: 0;
    }
    .navbar-links > :first-child {
        margin-inline-end: auto;
    }
    .navbar-link-home {
        display: flex;
        gap: 4px;
        align-items: center;
        padding: 8px;
        font-weight: 700;
    }

    .skip-navigation {
        top: 50%;
        transform: translate(8px, -50%);
        padding: 8px;
        background-color: var(--color-navbar-bg);
    }
</style>
