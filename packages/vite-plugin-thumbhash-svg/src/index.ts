import { createFilter } from '@rollup/pluginutils'
import { basename } from 'node:path'
import { relative } from 'node:path/posix'
import { readFile, access } from 'node:fs/promises'
import { constants } from 'node:fs'
import { extname } from 'node:path';

import { loadImage, createCanvas, ImageData } from '@napi-rs/canvas'
import { Resvg } from '@resvg/resvg-js'
import { rgbaToThumbHash, thumbHashToRGBA } from 'thumbhash-node'
import type { Plugin, ResolvedConfig } from 'vite'

export type OutputExtension = 'png' | 'jpg' | 'webp' | 'avif'

export type Options =
    | {
        include?: Array<string | RegExp> | string | RegExp
        exclude?: Array<string | RegExp> | string | RegExp
        outputExtension?: OutputExtension
    }
    | undefined

interface LoaderParams {
    thumbSrc: string
    thumbWidth: number
    thumbHeight: number
    originalSrc: string
    originalWidth: number
    originalHeight: number
}

const loader = (params: LoaderParams) => {
    return `export default ${JSON.stringify(params)}`
}

async function loadImageAndConvertToRgba(path: string) {
    const maxSize = 100
    const imgPath = path
    let image;
    //   console.log(path, extname(path))
    if (extname(path) === ".svg") {

        if (await exists(imgPath)) {
            const svg = await readFile(imgPath);
            const resvg = new Resvg(svg)
            const render = resvg.render()
            image = await loadImage(render.asPng())
        } else {
            throw new TypeError("Could not resolve image source!")
            // Try URI decode path?
            // console.log(decodeURIComponent(imgPath))
            // const svg = await readFile(decodeURIComponent(imgPath));
            // const resvg = new Resvg(svg)
            // const render = resvg.render()
            // image = await loadImage(render.asPng())

        }
    } else {
        // canvas handles all file loading for us
        image = await loadImage(imgPath)

    }
    const width = image.width
    const height = image.height

    const scale = maxSize / Math.max(width, height)
    const resizedWidth = Math.round(width * scale)
    const resizedHeight = Math.round(height * scale)

    const canvas = createCanvas(resizedWidth, resizedHeight)
    const ctx = canvas.getContext('2d')
    ctx.drawImage(image, 0, 0, resizedWidth, resizedHeight)

    const imageData = ctx.getImageData(0, 0, resizedWidth, resizedHeight)
    const rgba = new Uint8Array(imageData.data)

    return {
        originalWidth: width,
        originalHeight: height,
        height: imageData.height,
        width: imageData.width,
        rgba,
    }
}

const fromRGBAToImageBuffer = (
    rgba: Uint8Array,
    mimeType: MimeType,
    width: number,
    height: number
) => {
    const thumb = rgbaToThumbHash(width, height, rgba)
    const transformedRgba = thumbHashToRGBA(thumb)
    const imageData = new ImageData(
        new Uint8ClampedArray(transformedRgba.rgba),
        transformedRgba.width,
        transformedRgba.height
    )

    const canvas = createCanvas(transformedRgba.width, transformedRgba.height)
    const context = canvas.getContext('2d')
    //@ts-ignore
    context.putImageData(imageData, 0, 0)
    //@ts-ignore
    const buffer = canvas.toBuffer(mimeType)

    return buffer
}

type MimeType = 'image/webp' | 'image/jpeg' | 'image/avif' | 'image/png'

const extToMimeTypeMap: Record<OutputExtension, MimeType> = {
    avif: 'image/avif',
    jpg: 'image/jpeg',
    png: 'image/png',
    webp: 'image/webp',
}

const isThumbHash = (id: string) => {
    return id.endsWith('?th') || id.endsWith('?thumb')
}

const cleanId = (id: string) => decodeURIComponent(id.replace('?thumb', '').replace('?th', ''))

const buildViteAsset = (referenceId: string) => `__VITE_ASSET__${referenceId}__`

const buildDataURL = (buf: Buffer, mimeType: MimeType) => {
    const dataPrefix = `data:${mimeType};base64,`

    const dataURL = `${dataPrefix}${buf.toString('base64')}`

    return dataURL
}

async function exists(path: string) {
    try {
        await access(path, constants.F_OK)
        return true
    } catch {
        return false
    }
}

const thumbHash = (options: Options = {}): Plugin => {
    const { include, exclude, outputExtension = 'png' } = options

    const bufferMimeType = extToMimeTypeMap[outputExtension]

    const filter = createFilter(include, exclude)

    let config: ResolvedConfig

    const devCache = new Map<string, string>()

    const buildCache = new Map<string, string>()

    return {
        name: 'vite-plugin-thumbhash',
        enforce: 'pre',

        configResolved(cfg) {
            config = cfg
        },


        async load(id) {
            if (!filter(id)) {
                return null
            }
            
            if (id.includes("Design")) {
                console.log('raw', id)
            }

            if (isThumbHash(id)) {
                const cleanedId = cleanId(id)
                console.log('clean', cleanedId)

                if (config.command === 'serve') {
                    if (devCache.has(id)) {
                        return devCache.get(id)
                    }

                    const { rgba, width, height, originalHeight, originalWidth } =
                        await loadImageAndConvertToRgba(cleanedId)

                    const buffer = fromRGBAToImageBuffer(
                        rgba,
                        bufferMimeType,
                        width,
                        height
                    )

                    const dataURL = buildDataURL(buffer, bufferMimeType)

                    const loadedSource = loader({
                        thumbSrc: dataURL,
                        thumbWidth: width,
                        thumbHeight: height,
                        originalSrc: relative(config.root, cleanedId),
                        originalWidth: originalWidth,
                        originalHeight: originalHeight,
                    })

                    devCache.set(id, loadedSource)

                    return loadedSource
                }

                if (buildCache.has(id)) {
                    return buildCache.get(id)
                }

                const { rgba, width, height, originalHeight, originalWidth } =
                    await loadImageAndConvertToRgba(cleanedId)

                const buffer = fromRGBAToImageBuffer(
                    rgba,
                    bufferMimeType,
                    width,
                    height
                )

                const referenceId = this.emitFile({
                    type: 'asset',
                    name: basename(cleanedId).replace(
                        /\.(jpg)|(jpeg)|(png)|(webp)|(avif)/g,
                        `.${outputExtension}`
                    ),
                    source: buffer,
                })

                const originalRefId = this.emitFile({
                    type: 'asset',
                    name: basename(cleanedId),
                    source: await readFile(cleanedId),
                })

                // import.meta.ROLLUP_FILE_URL_

                const loadedSource = loader({
                    thumbSrc: buildViteAsset(referenceId),
                    thumbWidth: width,
                    thumbHeight: height,
                    originalSrc: buildViteAsset(originalRefId),
                    originalWidth: originalWidth,
                    originalHeight: originalHeight,
                })

                buildCache.set(id, loadedSource)

                return loadedSource
            }

            return null
        },
    }
}

export { thumbHash }