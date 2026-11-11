"use strict";
const { List, ActionPanel, Action, Icon, Color, showToast, Toast, getPreferenceValues, useNavigation, Detail } = require("@raycast/api");
const React = require("react");
const h = React.createElement;

const FRUITS = [
  { name: "Apple", family: "Rosaceae", color: Color.Red, emoji: "🍎", note: "Crisp, keeps for months." },
  { name: "Banana", family: "Musaceae", color: Color.Yellow, emoji: "🍌", note: "Ripens after picking." },
  { name: "Cherry", family: "Rosaceae", color: Color.Red, emoji: "🍒", note: "A stone fruit with a short season." },
  { name: "Grape", family: "Vitaceae", color: Color.Purple, emoji: "🍇", note: "Eaten fresh, dried or pressed." },
  { name: "Lime", family: "Rutaceae", color: Color.Green, emoji: "🍋", note: "Sharp and aromatic." },
  { name: "Orange", family: "Rutaceae", color: Color.Orange, emoji: "🍊", note: "A hybrid of pomelo and mandarin." },
];

function Fruit({ fruit }) {
  return h(Detail, {
    navigationTitle: fruit.name,
    markdown: `# ${fruit.emoji} ${fruit.name}\n\n${fruit.note}\n\n- Family: **${fruit.family}**\n- Pushed with \`Action.Push\``,
    actions: h(ActionPanel, null, h(Action.CopyToClipboard, { content: fruit.name })),
  });
}

exports.default = function Command() {
  const [count, setCount] = React.useState(0);
  const [showDetail, setShowDetail] = React.useState(true);
  const { greeting } = getPreferenceValues();
  const families = [...new Set(FRUITS.map((f) => f.family))];
  return h(List, { isShowingDetail: showDetail, searchBarPlaceholder: `Search ${greeting.toLowerCase()}...`, navigationTitle: "Fruits" },
    families.map((family) => h(List.Section, { key: family, title: family },
      FRUITS.filter((f) => f.family === family).map((fruit) => h(List.Item, {
        key: fruit.name,
        title: fruit.name,
        subtitle: fruit.family,
        icon: fruit.emoji,
        keywords: [fruit.family],
        accessories: [{ tag: { value: fruit.family, color: fruit.color } }, { text: `${count} taps` }],
        detail: h(List.Item.Detail, {
          markdown: `## ${fruit.emoji} ${fruit.name}\n\n${fruit.note}`,
          metadata: h(List.Item.Detail.Metadata, null,
            h(List.Item.Detail.Metadata.Label, { title: "Family", text: fruit.family }),
            h(List.Item.Detail.Metadata.Separator),
            h(List.Item.Detail.Metadata.Label, { title: "Taps", text: String(count) })),
        }),
        actions: h(ActionPanel, null,
          h(Action.Push, { title: "Show Fruit", icon: Icon.Eye, target: h(Fruit, { fruit }) }),
          h(Action, { title: "Tap", icon: Icon.Star, shortcut: { modifiers: ["cmd"], key: "t" }, onAction: async () => {
            setCount((c) => c + 1);
            await showToast({ style: Toast.Style.Success, title: "Tapped", message: fruit.name });
          } }),
          h(Action, { title: "Toggle Detail", icon: Icon.Sidebar, shortcut: { modifiers: ["cmd"], key: "d" }, onAction: () => setShowDetail((v) => !v) }),
          h(ActionPanel.Section, { title: "Copy" },
            h(Action.CopyToClipboard, { title: "Copy Name", content: fruit.name, shortcut: { modifiers: ["cmd", "shift"], key: "c" } }),
            h(Action.OpenInBrowser, { url: `https://en.wikipedia.org/wiki/${fruit.name}` }))),
      })))));
};
