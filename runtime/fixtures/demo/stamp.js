"use strict";
const { Clipboard, showHUD } = require("@raycast/api");

exports.default = async function Command() {
  const stamp = new Date().toISOString();
  await Clipboard.copy(stamp);
  await showHUD(`Copied ${stamp}`);
};
