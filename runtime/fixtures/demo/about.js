"use strict";
const { Detail, ActionPanel, Action, Color } = require("@raycast/api");
const h = require("react").createElement;

const MARKDOWN = `# EchoSpot Demo

This view is drawn from **Markdown** by the extension runtime.

## What it shows

- Headings, *emphasis*, **strong** text and \`inline code\`
- Lists and links: [the repository](https://github.com/adityavardhansharma/EchoSpot)

> Extensions run in their own Node process and speak to the palette in JSON.

\`\`\`js
export default function Command() {
  return <Detail markdown="# Hello" />;
}
\`\`\`
`;

exports.default = function Command() {
  return h(Detail, {
    markdown: MARKDOWN,
    navigationTitle: "About",
    metadata: h(Detail.Metadata, null,
      h(Detail.Metadata.Label, { title: "Runtime", text: `Node ${process.versions.node}` }),
      h(Detail.Metadata.TagList, { title: "Views" },
        h(Detail.Metadata.TagList.Item, { text: "List", color: Color.Blue }),
        h(Detail.Metadata.TagList.Item, { text: "Detail", color: Color.Green }),
        h(Detail.Metadata.TagList.Item, { text: "Form", color: Color.Orange })),
      h(Detail.Metadata.Separator),
      h(Detail.Metadata.Link, { title: "Source", text: "GitHub", target: "https://github.com/adityavardhansharma/EchoSpot" })),
    actions: h(ActionPanel, null, h(Action.CopyToClipboard, { title: "Copy Markdown", content: MARKDOWN })),
  });
};
