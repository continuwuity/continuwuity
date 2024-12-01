export async function GET({ url }) {
    const res = new Response(JSON.stringify({
        "fi.mau.bridges": [
            // "https://mautrix-discord.ellis.link",
            "https://mautrix-bluesky.ellis.link",
            "https://mautrix-gmessages.ellis.link",
            "https://mautrix-meta.ellis.link",
            "https://mautrix-signal.ellis.link",
            "https://mautrix-slack.ellis.link",
            "https://mautrix-whatsapp.ellis.link"
        ]
    }), { headers: { "content-type": "application/jrd+json" }, status: 200 })
    return res;
}