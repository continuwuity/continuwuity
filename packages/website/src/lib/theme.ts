import { writable } from 'svelte/store'

const query = typeof window != "undefined" ? window?.matchMedia('(prefers-color-scheme: dark)') : undefined

export const theme = writable(query?.matches ? 'dark' : 'light')

query?.addEventListener('change', e => {
    theme.set(e.matches ? 'dark' : 'light')
});
