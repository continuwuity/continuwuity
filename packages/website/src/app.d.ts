// See https://kit.svelte.dev/docs/types#app
// for information about these interfaces
import type { Middleware } from "polka"
type Req = Parameters<Middleware>[0]
declare global {
	namespace App {
		// interface Error {}
		// interface Locals {}
		// interface PageData {}
		// interface PageState {}
		interface Platform {
            req: Req
        }
	}
}

export {};
