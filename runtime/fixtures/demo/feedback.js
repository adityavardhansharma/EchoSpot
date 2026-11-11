"use strict";
const { Form, ActionPanel, Action, showHUD, popToRoot } = require("@raycast/api");
const React = require("react");
const h = React.createElement;

exports.default = function Command() {
  const [error, setError] = React.useState();
  return h(Form, {
    navigationTitle: "Send Feedback",
    actions: h(ActionPanel, null, h(Action.SubmitForm, { title: "Send Feedback", onSubmit: async (values) => {
      if (!values.subject) return setError("Say what this is about");
      await showHUD(`Sent: ${values.subject} (${values.kind}${values.urgent ? ", urgent" : ""})`);
      await popToRoot();
    } })),
  },
    h(Form.Description, { title: "Feedback", text: "Nothing leaves this machine; the form only exercises the runtime." }),
    h(Form.TextField, { id: "subject", title: "Subject", placeholder: "One line", error, onChange: () => setError(undefined) }),
    h(Form.TextArea, { id: "body", title: "Details", placeholder: "What happened?" }),
    h(Form.Dropdown, { id: "kind", title: "Kind", defaultValue: "idea" },
      h(Form.Dropdown.Item, { value: "bug", title: "Bug" }),
      h(Form.Dropdown.Item, { value: "idea", title: "Idea" }),
      h(Form.Dropdown.Item, { value: "praise", title: "Praise" })),
    h(Form.Separator),
    h(Form.Checkbox, { id: "urgent", label: "This is urgent", defaultValue: false }),
    h(Form.PasswordField, { id: "token", title: "Token", placeholder: "Optional" }));
};
