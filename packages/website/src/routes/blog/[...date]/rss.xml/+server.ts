
export async function GET({ params, url}) {
    // Redirect to the rss feed with the date from the route
    let date = params.date;
    let newUrl = new URL("/blog/rss.xml", url);
    newUrl.searchParams.set("date", date);
    return new Response(null, {
        status: 301,
        headers: {
            location: newUrl.toString(),
        },
    });
}