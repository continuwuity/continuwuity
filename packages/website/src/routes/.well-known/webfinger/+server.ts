import type { RequestHandler, RequestEvent } from '@sveltejs/kit';

export interface Ijrd {
    properties?: {
        [link: string]: string | null;
    };
    links: IjrdLink[];
    subject?: string;
    aliases?: string[];
    error?: any;
}

export interface IjrdLink {
    rel: string;
    type?: string;
    href?: string;
    titles?: [{
        [languageTag: string]: string;
    }];
    properties?: {
        [link: string]: string | null;
    };
    [key: string]: any;
}

const accounts = [
    "JadedBlueEyes@tech.lgbt",
    "jade@ellis.link",
].map(i => i.toLowerCase())
export async function GET({ url }: RequestEvent) {
    // export const GET = async ({ url }) => {
    let resource = url.searchParams.get("resource");
    if (resource?.split(":")[0] !== "acct") {
        let res = new Response("", { status: 404 })
        return res;
    }
    let account = resource?.split(":")[1]
    if (!accounts.includes(account.toLowerCase()) && !account.toLowerCase().endsWith("@jade.ellis.link")) {
        if (resource?.split(":")[0] !== "acct") {
            let res = new Response("", { status: 404 })
            return res;
        }
    }

    const webFinger = {
        "subject": "acct:" + account,
        "aliases": [
            "https://tech.lgbt/@JadedBlueEyes",
            "https://tech.lgbt/users/JadedBlueEyes"
        ],
        "links": [
            {
                "rel": "http://webfinger.net/rel/profile-page",
                "type": "text/html",
                "href": "https://tech.lgbt/@JadedBlueEyes"
            },
            {
                "rel": "self",
                "type": "application/activity+json",
                "href": "https://tech.lgbt/users/JadedBlueEyes"
            },
            {
                "rel": "http://ostatus.org/schema/1.0/subscribe",
                "template": "https://tech.lgbt/authorize_interaction?uri={uri}"
            },
            {
                "rel": "http://webfinger.net/rel/avatar",
                "type": "image/png",
                "href": "https://media.tech.lgbt/accounts/avatars/110/022/214/816/752/685/original/087ccc2173ffd8e0.png"
            },
            {
              "rel": "me",
              "href": "https://jade.ellis.link"
            }
        ]
    }
    // const isMe = (user.toLowerCase() == EMAIL.toLowerCase()) ? true : false;
        let res = new Response(JSON.stringify(webFinger), { headers: { "content-type": "application/jrd+json" }, status: 200 })
        return res;
}
