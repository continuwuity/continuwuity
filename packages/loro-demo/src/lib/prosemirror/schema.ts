
import { Schema } from 'prosemirror-model';
import { nodes, marks } from 'prosemirror-schema-basic';
  

export const EXPAND_CONFIG: { [key in string]: { expand: "before" | "after" | "both" | "none"; } } = {
    bold: { expand: "after" },
    italic: { expand: "after" },
    underline: { expand: "after" },
    link: { expand: "none" },
    heading: { expand: "none" },
}



/**
 * Schema to represent rich text
 * @type {Schema}
 */
export const richTextSchema = new Schema({
    nodes,
    marks
  });
// richTextSchema.nodes.heading