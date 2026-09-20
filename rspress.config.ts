import { defineConfig } from '@rspress/core';
import { pluginSitemap } from '@rspress/plugin-sitemap';
import { pluginClientRedirects } from '@rspress/plugin-client-redirects';

export default defineConfig({
    root: 'docs',
    title: 'Continuwuity',
    description: 'A community-driven Matrix homeserver',
    icon: '/assets/logo.svg',
    logo: {
        light: '/assets/logo.svg',
        dark: '/assets/logo.svg',
    },
    markdown: {
        link: {
            checkDeadLinks: {
                excludes: [
                    '/deploying/docker-compose.with-caddy.yml',
                    '/deploying/docker-compose.with-caddy-labels.yml',
                    '/deploying/docker-compose.for-traefik.yml',
                    '/deploying/docker-compose.with-traefik.yml',
                    `/deploying/docker-compose.override.yml`,
                    `/deploying/docker-compose.yml`,
                    '/advanced/delegated.docker-compose.with-caddy.yml',
                    '/advanced/delegated.docker-compose.with-caddy-labels.yml',
                    '/advanced/delegated.docker-compose.for-traefik.yml',
                    '/advanced/delegated.docker-compose.with-traefik.yml',
                    '/calls/livekit.docker-compose.with-caddy-labels.yml',
                    '/calls/livekit.nginx.conf'
                ]
            },
        },
    },
    themeConfig: {
        socialLinks: [
            {
                icon: {
                    svg: `<svg role="img" viewBox="0 0 24 24" width="100%" xmlns="http://www.w3.org/2000/svg"><title>Matrix</title><path fill="currentColor" d="M.632.55v22.9H2.28V24H0V0h2.28v.55zm7.043 7.26v1.157h.033c.309-.443.683-.784 1.117-1.024.433-.245.936-.365 1.5-.365.54 0 1.033.107 1.481.314.448.208.785.582 1.02 1.108.254-.374.6-.706 1.034-.992.434-.287.95-.43 1.546-.43.453 0 .872.056 1.26.167.388.11.716.286.993.53.276.245.489.559.646.951.152.392.23.863.23 1.417v5.728h-2.349V11.52c0-.286-.01-.559-.032-.812a1.755 1.755 0 0 0-.18-.66 1.106 1.106 0 0 0-.438-.448c-.194-.11-.457-.166-.785-.166-.332 0-.6.064-.803.189a1.38 1.38 0 0 0-.48.499 1.946 1.946 0 0 0-.231.696 5.56 5.56 0 0 0-.06.785v4.768h-2.35v-4.8c0-.254-.004-.503-.018-.752a2.074 2.074 0 0 0-.143-.688 1.052 1.052 0 0 0-.415-.503c-.194-.125-.476-.19-.854-.19-.111 0-.259.024-.439.074-.18.051-.36.143-.53.282-.171.138-.319.337-.439.595-.12.259-.18.6-.18 1.02v4.966H5.46V7.81zm15.693 15.64V.55H21.72V0H24v24h-2.28v-.55z"/></svg>`
                },
                mode: 'link',
                content: 'https://matrix.to/#/#continuwuity:continuwuity.org',
            },
            {
                icon: {
                    svg: `<svg role="img" viewBox="0 0 24 24" width="100%" xmlns="http://www.w3.org/2000/svg"><title>Forgejo</title><path fill="currentColor" d="M16.7773 0c1.6018 0 2.9004 1.2986 2.9004 2.9005s-1.2986 2.9004-2.9004 2.9004c-1.0854 0-2.0315-.596-2.5288-1.4787H12.91c-2.3322 0-4.2272 1.8718-4.2649 4.195l-.0007 2.1175a7.0759 7.0759 0 0 1 4.148-1.4205l.1176-.001 1.3385.0002c.4973-.8827 1.4434-1.4788 2.5288-1.4788 1.6018 0 2.9004 1.2986 2.9004 2.9005s-1.2986 2.9004-2.9004 2.9004c-1.0854 0-2.0315-.596-2.5288-1.4787H12.91c-2.3322 0-4.2272 1.8718-4.2649 4.195l-.0007 2.319c.8827.4973 1.4788 1.4434 1.4788 2.5287 0 1.602-1.2986 2.9005-2.9005 2.9005-1.6018 0-2.9004-1.2986-2.9004-2.9005 0-1.0853.596-2.0314 1.4788-2.5287l-.0002-9.9831c0-3.887 3.1195-7.0453 6.9915-7.108l.1176-.001h1.3385C14.7458.5962 15.692 0 16.7773 0ZM7.2227 19.9052c-.6596 0-1.1943.5347-1.1943 1.1943s.5347 1.1943 1.1943 1.1943 1.1944-.5347 1.1944-1.1943-.5348-1.1943-1.1944-1.1943Zm9.5546-10.4644c-.6596 0-1.1944.5347-1.1944 1.1943s.5348 1.1943 1.1944 1.1943c.6596 0 1.1943-.5347 1.1943-1.1943s-.5347-1.1943-1.1943-1.1943Zm0-7.7346c-.6596 0-1.1944.5347-1.1944 1.1943s.5348 1.1943 1.1944 1.1943c.6596 0 1.1943-.5347 1.1943-1.1943s-.5347-1.1943-1.1943-1.1943Z"/></svg>`
                },
                mode: 'link',
                content: 'https://forgejo.ellis.link/continuwuation/continuwuity'
            },
            {
                icon: 'github',
                mode: 'link',
                content: 'https://github.com/continuwuity/continuwuity',
            },
            {
                icon: {
                    svg: `<svg role="img" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><title>Open Collective</title><path fill="currentColor" d="M12 0C5.373 0 0 5.373 0 12s5.373 12 12 12c2.54 0 4.894-.79 6.834-2.135l-3.107-3.109a7.715 7.715 0 1 1 0-13.512l3.107-3.109A11.943 11.943 0 0 0 12 0zm9.865 5.166l-3.109 3.107A7.67 7.67 0 0 1 19.715 12a7.682 7.682 0 0 1-.959 3.727l3.109 3.107A11.943 11.943 0 0 0 24 12c0-2.54-.79-4.894-2.135-6.834z"/></svg>`
                },
                mode: 'link',
                content: 'https://opencollective.com/continuwuity',
            },
        ],
        lastUpdated: true,
        enableContentAnimation: true,
        enableAppearanceAnimation: false,
        footer: {
        },
    },

    plugins: [pluginSitemap({
        siteUrl: 'https://continuwuity.org', // TODO: Set automatically in build pipeline
    }),
    pluginClientRedirects({
        redirects: [{
            from: '^/config_reference$',
            to: '/reference/config'
        }, {
            from: '^/admin_reference$',
            to: '/reference/admin'
        }, {
            from: '^/community$',
            to: '/community/guidelines'
        }, {
            from: "^/turn$",
            to: "/calls/turn",
        }, {
            from: "^/livekit$",
            to: "/calls/livekit",
        }, {
            from: "^/dns$",
            to: "/guides/dns",
        }, {
            from: "^/source(|_code)$",
            to: "https://forgejo.ellis.link/continuwuation/continuwuity",
        }
        ]
    })],
});
