
export interface MdsvexPage {
    readingTime: ReadingTime
    flattenedHeadings: FlattenedHeading[]
    headings: NestedHeading[]
    [key: string]: unknown
}


export interface ReadingTime {
    text: string
    minutes: number
    time: number
    words: number
}

export interface FlattenedHeading {
    level: number
    title: string
    id: string
}

export interface NestedHeading {
    level: number
    title: string
    id: string
    children: NestedHeading[]
}
