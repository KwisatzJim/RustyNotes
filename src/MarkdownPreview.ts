import { Component, createElement as h, memo, type ReactNode } from "react";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

export const MAX_PREVIEW_CHARACTERS = 100_000;
const components: Components = {
  // No href, src, event handlers, or arbitrary properties from note content.
  a: ({ children, href }) => h("span", { className: "preview-link", title: href ? `${href} (link navigation disabled in preview)` : "Link navigation disabled in preview" }, children),
  img: ({ alt }) => h("span", { className: "preview-image" }, `[Image: ${alt || "no description"} — not loaded]`),
  input: ({ checked }) => h("input", { type: "checkbox", checked: !!checked, disabled: true, readOnly: true, "aria-label": checked ? "Completed task" : "Incomplete task" }),
};
const allowed = ["p", "h1", "h2", "h3", "h4", "h5", "h6", "br", "hr", "blockquote", "ul", "ol", "li", "strong", "em", "del", "a", "img", "pre", "code", "table", "thead", "tbody", "tr", "th", "td", "input", "section", "sup"];

export const MarkdownPreview = memo(function MarkdownPreview({ content }: { content: string }) {
  if (content.length > MAX_PREVIEW_CHARACTERS) {
    return h("p", { role: "status" }, "This note is too large to preview (limit: 100,000 characters). Switch to Edit to read or change the full Markdown. Your note is unchanged.");
  }
  if (!content.trim()) return h("p", { className: "preview-empty" }, "This note is empty. Switch to Edit to add text.");
  return h(Markdown, { remarkPlugins: [remarkGfm], skipHtml: true, allowedElements: allowed, components, children: content });
});

export class PreviewBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  render() {
    return this.state.failed
      ? h("p", { role: "alert" }, "This note could not be previewed. Switch to Edit to read its Markdown. Your note is unchanged.")
      : this.props.children;
  }
}
