
import { tags as t } from '@lezer/highlight';

// NOTE: This requires enabling unsafe-inline styles in the CSP
// From thememirror
import { EditorView } from '@codemirror/view';
import type { Extension } from '@codemirror/state';
import {
	HighlightStyle,
	type TagStyle,
	syntaxHighlighting,
} from '@codemirror/language';

interface Options {
	/**
	 * Theme variant. Determines which styles CodeMirror will apply by default.
	 */
	variant: Variant;

	/**
	 * Settings to customize the look of the editor, like background, gutter, selection and others.
	 */
	settings: Settings;

	/**
	 * Syntax highlighting styles.
	 */
	styles: TagStyle[];
}

type Variant = 'light' | 'dark';

interface Settings {
	/**
	 * Editor background.
	 */
	background: string;

	/**
	 * Default text color.
	 */
	foreground: string;

	/**
	 * Caret color.
	 */
	caret: string;

	/**
	 * Selection background.
	 */
	selection: string;

	/**
	 * Background of highlighted lines.
	 */
	lineHighlight: string;

	/**
	 * Gutter background.
	 */
	gutterBackground: string;

	/**
	 * Text color inside gutter.
	 */
	gutterForeground: string;
}

const createTheme = ({ variant, settings, styles }: Options): Extension => {
	const theme = EditorView.theme(
		{
			// eslint-disable-next-line @typescript-eslint/naming-convention
			'&': {
				backgroundColor: settings.background,
				color: settings.foreground,
			},
			'.cm-content': {
				caretColor: settings.caret,
			},
			'.cm-cursor, .cm-dropCursor': {
				borderLeftColor: settings.caret,
			},
			'&.cm-focused .cm-selectionLayer .cm-selectionBackground, .cm-content ::selection':
			{
				backgroundColor: settings.selection,
			},
			'.cm-activeLine': {
				backgroundColor: settings.lineHighlight,
			},
			'.cm-gutters': {
				backgroundColor: settings.gutterBackground,
				color: settings.gutterForeground,
			},
			'.cm-activeLineGutter': {
				backgroundColor: settings.lineHighlight,
			},
		},
		{
			dark: variant === 'dark',
		},
	);

	const highlightStyle = HighlightStyle.define(styles);
	const extension = [theme, syntaxHighlighting(highlightStyle)];

	return extension;
};

export default createTheme;

export const githubLight = createTheme({
	variant: 'light',
	settings: {
		background: '#fff',
		foreground: '#24292e',
		selection: '#BBDFFF',
		// selectionMatch: '#BBDFFF',
		gutterBackground: '#fff',
		gutterForeground: '#6e7781',
		caret: '#7c3aed',
		lineHighlight: '#8a91991a',
	},
	styles: [
		{ tag: [t.standard(t.tagName), t.tagName], color: '#116329' },
		{ tag: [t.comment, t.bracket], color: '#6a737d' },
		{ tag: [t.className, t.propertyName], color: '#6f42c1' },
		{ tag: [t.variableName, t.attributeName, t.number, t.operator], color: '#005cc5' },
		{ tag: [t.keyword, t.typeName, t.typeOperator, t.typeName], color: '#d73a49' },
		{ tag: [t.string, t.meta, t.regexp], color: '#032f62' },
		{ tag: [t.name, t.quote], color: '#22863a' },
		{ tag: [t.heading, t.strong], color: '#24292e', fontWeight: 'bold' },
		{ tag: [t.emphasis], color: '#24292e', fontStyle: 'italic' },
		{ tag: [t.deleted], color: '#b31d28', backgroundColor: 'ffeef0' },
		{ tag: [t.atom, t.bool, t.special(t.variableName)], color: '#e36209' },
		{ tag: [t.url, t.escape, t.regexp, t.link], color: '#032f62' },
		{ tag: t.link, textDecoration: 'underline' },
		{ tag: t.strikethrough, textDecoration: 'line-through' },
		{ tag: t.invalid, color: '#cb2431' }
	],
});

export
	const githubDark = createTheme({
		variant: 'dark',
		settings: {
			background: '#161616',
			foreground: '#d8d8d8',
			caret: '#c9d1d9',
			selection: '#003d73',
			// selectionMatch: '#003d73',\
			lineHighlight: '#1e1e1e',
			gutterBackground: '#1c1c1c',
			gutterForeground: '#fff',
		},
		styles: [
			{ tag: [t.standard(t.tagName), t.tagName], color: '#7ee787' },
			{ tag: [t.comment, t.bracket], color: '#8b949e' },
			{ tag: [t.className, t.propertyName], color: '#d2a8ff' },
			{ tag: [t.variableName, t.attributeName, t.number, t.operator], color: '#79c0ff' },
			{ tag: [t.keyword, t.typeName, t.typeOperator, t.typeName], color: '#ff7b72' },
			{ tag: [t.string, t.meta, t.regexp], color: '#a5d6ff' },
			{ tag: [t.name, t.quote], color: '#7ee787' },
			{ tag: [t.heading, t.strong], color: '#d2a8ff', fontWeight: 'bold' },
			{ tag: [t.emphasis], color: '#d2a8ff', fontStyle: 'italic' },
			{ tag: [t.deleted], color: '#ffdcd7', backgroundColor: 'ffeef0' },
			{ tag: [t.atom, t.bool, t.special(t.variableName)], color: '#ffab70' },
			{ tag: t.link, textDecoration: 'underline' },
			{ tag: t.strikethrough, textDecoration: 'line-through' },
			{ tag: t.invalid, color: '#f97583' },
		],
	});