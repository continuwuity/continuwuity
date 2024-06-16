

// import MagicString from "magic-string";
import { Parser } from "acorn";
import { type MinifyOptions, type MinifyOutput } from "terser";

let sourceMap = false;

import { configSchema } from "./config.schema";
import type { Config } from "./config";
// console.log(configSchema)
export async function bookmarkify(code: string, options: Config, minify: (files: string | string[] | {
    [file: string]: string;
}, options?: MinifyOptions | undefined) => Promise<MinifyOutput>
) {

    // try {
    if (options.script) {
        options.script = options.script.reverse();
        options.script.forEach(s => {
            let { path, opts } = extractOptions(s);
            code = loadScript(code, path, opts.loadOnce);
        });
    }

    if (options.style) {
        options.style.forEach(s => {
            let { path, opts } = extractOptions(s);
            code = loadStyle(path, opts.loadOnce) + code;
        });
    }

    const result = await minify(code, { sourceMap });
    // return result.code;
    if (typeof result.code == "string") {
        // const intermediate =  new MagicString(result.code);
        return `javascript:${encodeURIComponent("(function(){" + result.code + "})()")}`;
    }
    // } catch (e) {
    //     console.log("Error occurred", e);
    // }
}

export async function parseMeta(str: string): Promise<Config> {
    enum MetaState {
        PreOpen,
        Opened,
        Closed
    }
    let state: MetaState = MetaState.PreOpen


    const openMetadata = /==bookmarklet==/gim;
    const closeMetadata = /==\/bookmarklet==/gim;
    const metaLine = /^[\s]*@([^\s]+)\s+(.*)$/gim;
    let options: Config = {};

    Parser.parse(str, {
        ecmaVersion: "latest",
        onComment(isBlock, text, start, end, startLoc, endLoc) {
            openMetadata.lastIndex = 0;
            closeMetadata.lastIndex = 0;
            metaLine.lastIndex = 0;
            if (state == MetaState.PreOpen) {
                let res = openMetadata.exec(text)
                if (res !== null) {
                    state = MetaState.Opened
                    closeMetadata.lastIndex = openMetadata.lastIndex;
                    metaLine.lastIndex = openMetadata.lastIndex;
                    // console.log("Meta opened at", start + openMetadata.lastIndex)
                }
            }
            // console.log(text, closeMetadata.lastIndex)
            let res
            while (state == MetaState.Opened && (res = metaLine.exec(text)) !== null) {
                closeMetadata.lastIndex = metaLine.lastIndex;
                // console.log(str.slice(start + 2 + (metaLine.lastIndex - res[0].length), start + 2 + metaLine.lastIndex ))
                let k = res[1];
                let v = res[2];
                if (k) {
                    if (configSchema.properties[k]?.type == "array") {
                        options[k] = options[k] || [];
                        options[k].push(v);
                    } else if (configSchema.properties[k]?.type == "boolean") {
                        options[k] = v.toLowerCase() == 'true';
                    } else {
                        options[k] = v;
                    }
                }
            }

            if (state == MetaState.Opened) {
                let endRes = closeMetadata.exec(text)
                if (endRes !== null) {
                    state = MetaState.Closed;

                    // console.log("Meta closed at", start + closeMetadata.lastIndex)
                }
            }
        },
    });

    // @ts-ignore
    if (state == MetaState.Opened) {
        throw new Error("Missing metadata close block. Add '==/Bookmarklet==' to your comment block");
        
    }
    return options;
}

function loadScript(code: string, path: string, loadOnce: boolean) {
    loadOnce = !!loadOnce;
    let id = `bookmarklet__script_${cyrb53(path).toString(36).substring(0, 7)}`;
    return `
          function callback(){
            ${code}
          }
  
          if (!${loadOnce} || !document.getElementById("${id}")) {
            var s = document.createElement("script");
            if (s.addEventListener) {
              s.addEventListener("load", callback, false)
            } else if (s.readyState) {
              s.onreadystatechange = callback
            }
            if (${loadOnce}) {
              s.id = "${id}";
            }
            s.src = "${quoteEscape(path)}";
            document.body.appendChild(s);
          } else {
            callback();
          }
      `;
}

function loadStyle(path: string, loadOnce: boolean) {
    loadOnce = !!loadOnce;
    let id = `bookmarklet__style_${cyrb53(path).toString(36).substring(0, 7)}`;
    return `
          if (!${loadOnce} || !document.getElementById("${id}")) {
            var link = document.createElement("link");
            if (${loadOnce}) {
              link.id = "${id}";
            }
            link.rel="stylesheet";
            link.href = "${quoteEscape(path)}";
            document.body.appendChild(link);
          }
      `;
}
const cyrb53 = (str: string, seed = 0) => {
    let h1 = 0xdeadbeef ^ seed, h2 = 0x41c6ce57 ^ seed;
    for (let i = 0, ch; i < str.length; i++) {
        ch = str.charCodeAt(i);
        h1 = Math.imul(h1 ^ ch, 2654435761);
        h2 = Math.imul(h2 ^ ch, 1597334677);
    }
    h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507);
    h1 ^= Math.imul(h2 ^ (h2 >>> 13), 3266489909);
    h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507);
    h2 ^= Math.imul(h1 ^ (h1 >>> 13), 3266489909);

    return 4294967296 * (2097151 & h2) + (h1 >>> 0);
};
function extractOptions(path: string) {
    // Returns {
    //   path: the updated path string (minus any options)
    //   opts: plain object of options
    // }
    //
    // You can prefix a path with options in the form of:
    //
    // ```
    // @style !loadOnce !foo=false https://example.com/foo.css
    // ```
    //
    // If there is no `=`, then the value of the option defaults to `true`.
    // Values get converted via JSON.parse if possible, o/w they're a string.
    //
    let opts: { [x: string]: any } = {};

    let matcher = /^(\![^\s]+)\s+/g

    let m
    let splitAfter = 0;
    while ((m = matcher.exec(path)) !== null) {
        splitAfter = matcher.lastIndex;

        let opt = m[1].substring(1).split('=');
        opts[opt[0]] = opt[1] === undefined ? true : _fuzzyParse(opt[1]);
        // break
    }
    return { path: path.substring(splitAfter), opts };
}

const _fuzzyParse = (val: string) => {
    try {
        return JSON.parse(val);
    } catch (e) {
        return val;
    }
};

// function result() {

//     return minification(value)
//         .then((result) => {
//             errorMessage = "";
//             if (result === "") {
//                 errorMessage = "Put some code in there!";
//             } else {
//                 codeOutput = "javascript:(function(){" + result + "}());";
//             }
//             return;
//         })
//         .catch((err) => {
//             codeOutput = "";
//             return (errorMessage = err);
//         });
// }


function quoteEscape(x) {
    return x.replace('"', '\\"').replace("'", "\\'");
}