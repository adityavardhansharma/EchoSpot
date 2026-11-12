// The `@raycast/api` an extension sees. Components render to named host nodes; system calls are
// done here with ordinary Linux tools, and only what needs the palette itself goes to EchoSpot.

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import crypto from "node:crypto";
import { execFile, execFileSync, spawn } from "node:child_process";
import React from "react";
import { SLOT } from "./reconciler.js";
import * as enums from "./enums.generated.js";

const h = React.createElement;

/// A component that renders as one host node, lifting element-valued props into slots.
function host(type, slots = {}) {
  const Component = (props) => {
    const own = {};
    const lifted = [];
    for (const key of Object.keys(props)) {
      if (key === "children") continue;
      if (key in slots) {
        if (props[key] != null) lifted.push(h(SLOT, { key: `slot:${key}`, name: key, single: slots[key] === "single" }, props[key]));
      } else {
        own[key] = props[key];
      }
    }
    return h(type, own, props.children, ...lifted);
  };
  Component.displayName = type;
  return Component;
}

export function createApi(ctx) {
  const { extensionDir, manifest, command, launch, call, notify } = ctx;
  const home = os.homedir();
  const configHome = process.env.XDG_CONFIG_HOME || path.join(home, ".config");
  const dataHome = process.env.XDG_DATA_HOME || path.join(home, ".local/share");
  const extensionName = manifest.name;
  const supportPath = path.join(dataHome, "echospot/extensions-data", extensionName);
  const prefsPath = path.join(configHome, "echospot/extensions", `${extensionName}.json`);
  fs.mkdirSync(supportPath, { recursive: true });

  // Work the command started and did not await (a toast, a clipboard write) still finishes.
  const inFlight = new Set();
  const track = (promise) => {
    inFlight.add(promise);
    promise.finally(() => inFlight.delete(promise));
    return promise;
  };

  const run = (program, args, input) => track(new Promise((resolve, reject) => {
    const child = execFile(program, args, { maxBuffer: 64 * 1024 * 1024 }, (error, stdout) => (error ? reject(error) : resolve(stdout)));
    if (input !== undefined) child.stdin.end(input);
  }));
  const detached = (program, args) => {
    const child = spawn(program, args, { detached: true, stdio: "ignore" });
    child.on("error", () => {});
    child.unref();
  };

  // ---- Preferences -------------------------------------------------------------------------
  function preferences() {
    let saved = {};
    try {
      saved = JSON.parse(fs.readFileSync(prefsPath, "utf8"));
    } catch {}
    const out = {};
    const declared = [...(manifest.preferences || []), ...(command.preferences || [])];
    for (const pref of declared) {
      const value = saved[`${command.name}.${pref.name}`] ?? saved[pref.name] ?? pref.default;
      if (value === undefined) continue;
      out[pref.name] = pref.type === "checkbox" ? value === true || value === "true" : value;
    }
    return out;
  }

  // ---- Storage -----------------------------------------------------------------------------
  const storagePath = path.join(supportPath, "local-storage.json");
  const readStore = () => {
    try {
      return JSON.parse(fs.readFileSync(storagePath, "utf8"));
    } catch {
      return {};
    }
  };
  const writeStore = (data) => fs.writeFileSync(storagePath, JSON.stringify(data));
  const LocalStorage = {
    getItem: async (key) => readStore()[key],
    setItem: async (key, value) => {
      const data = readStore();
      data[key] = value;
      writeStore(data);
    },
    removeItem: async (key) => {
      const data = readStore();
      delete data[key];
      writeStore(data);
    },
    allItems: async () => readStore(),
    clear: async () => writeStore({}),
  };

  class Cache {
    constructor(options = {}) {
      this.file = path.join(supportPath, `cache-${options.namespace || "default"}.json`);
      this.subscribers = new Set();
      try {
        this.data = JSON.parse(fs.readFileSync(this.file, "utf8"));
      } catch {
        this.data = {};
      }
    }
    get isEmpty() {
      return Object.keys(this.data).length === 0;
    }
    get(key) {
      return this.data[key];
    }
    has(key) {
      return key in this.data;
    }
    #save(key, value) {
      try {
        fs.writeFileSync(this.file, JSON.stringify(this.data));
      } catch {}
      for (const subscriber of this.subscribers) subscriber(key, value);
    }
    set(key, value) {
      this.data[key] = value;
      this.#save(key, value);
    }
    remove(key) {
      const had = key in this.data;
      delete this.data[key];
      this.#save(key, undefined);
      return had;
    }
    clear() {
      this.data = {};
      this.#save(undefined, undefined);
    }
    subscribe(subscriber) {
      this.subscribers.add(subscriber);
      return () => this.subscribers.delete(subscriber);
    }
  }

  // ---- Clipboard ---------------------------------------------------------------------------
  const contentText = (content) => {
    if (content == null) return "";
    if (typeof content === "string" || typeof content === "number") return String(content);
    return content.text ?? content.html ?? content.file ?? "";
  };
  const Clipboard = {
    copy: async (content, options) => {
      if (content && typeof content === "object" && content.file) {
        await run("wl-copy", ["--type", "text/uri-list"], `file://${content.file}\n`);
      } else {
        const args = options?.concealed ? ["--sensitive"] : [];
        await run("wl-copy", args, contentText(content)).catch(() => run("wl-copy", [], contentText(content)));
      }
    },
    paste: async (content) => {
      await Clipboard.copy(content);
      notify("paste", []);
    },
    clear: async () => {
      await run("wl-copy", ["--clear"]);
    },
    readText: async () => {
      try {
        return (await run("wl-paste", ["--no-newline"])) || undefined;
      } catch {
        return undefined;
      }
    },
    read: async () => ({ text: (await Clipboard.readText()) ?? "" }),
  };

  // ---- Feedback ----------------------------------------------------------------------------
  let toastIds = 0;
  class Toast {
    constructor(options) {
      this.id = ++toastIds;
      this.options = { style: "SUCCESS", ...options };
    }
    get title() {
      return this.options.title;
    }
    set title(value) {
      this.options.title = value;
      this.show();
    }
    get message() {
      return this.options.message;
    }
    set message(value) {
      this.options.message = value;
      this.show();
    }
    get style() {
      return this.options.style;
    }
    set style(value) {
      this.options.style = value;
      this.show();
    }
    set primaryAction(value) {
      this.options.primaryAction = value;
    }
    set secondaryAction(value) {
      this.options.secondaryAction = value;
    }
    async show() {
      notify("toast", [{ id: this.id, style: this.options.style, title: String(this.options.title ?? ""), message: this.options.message ? String(this.options.message) : "" }]);
    }
    async hide() {
      notify("hideToast", [this.id]);
    }
  }
  Toast.Style = enums.ToastStyle;
  async function showToast(first, title, message) {
    const options = typeof first === "object" && first !== null ? first : { style: first, title, message };
    const toast = new Toast(options);
    await toast.show();
    return toast;
  }
  const showHUD = async (title, options) => {
    notify("hud", [String(title)]);
    if (options?.clearRootSearch) notify("clearSearchBar", []);
  };

  const Alert = { ActionStyle: enums.AlertActionStyle };
  const confirmAlert = async (options) => {
    const confirmed = await call("confirm", [{
      title: String(options.title ?? ""), message: options.message ? String(options.message) : "",
      confirm: options.primaryAction?.title || "OK",
      destructive: options.primaryAction?.style === "destructive",
    }]);
    if (confirmed) await options.primaryAction?.onAction?.();
    else await options.dismissAction?.onAction?.();
    return Boolean(confirmed);
  };

  // ---- OAuth -------------------------------------------------------------------------------
  // The provider sends the browser back to Raycast's redirect page, which opens a
  // `raycast://oauth` link; EchoSpot is that link's handler and answers the waiting `authorize`.
  const base64url = (buffer) => Buffer.from(buffer).toString("base64url");
  const redirectFor = (method) => {
    if (method === "app") return "raycast://oauth?package_name=Extension";
    if (method === "appURI") return "com.raycast:/oauth?package_name=Extension";
    return "https://raycast.com/redirect?packageName=Extension";
  };
  class TokenSet {
    constructor(data) {
      this.accessToken = data.accessToken;
      this.refreshToken = data.refreshToken;
      this.idToken = data.idToken;
      this.expiresIn = data.expiresIn;
      this.scope = data.scope;
      this.updatedAt = new Date(data.updatedAt || Date.now());
    }
    isExpired() {
      if (this.expiresIn == null) return false;
      // Ten seconds early, so a token is not handed over as it lapses.
      return this.updatedAt.getTime() + (Number(this.expiresIn) - 10) * 1000 < Date.now();
    }
  }
  class PKCEClient {
    constructor(options = {}) {
      this.redirectMethod = options.redirectMethod;
      this.providerName = options.providerName || extensionName;
      this.providerIcon = options.providerIcon;
      this.providerId = options.providerId;
      this.description = options.description;
      const id = String(this.providerId || this.providerName).toLowerCase().replace(/[^a-z0-9]+/g, "-");
      this.tokensPath = path.join(supportPath, `oauth-${id}.json`);
    }
    async authorizationRequest(options) {
      const codeVerifier = base64url(crypto.randomBytes(48));
      const codeChallenge = base64url(crypto.createHash("sha256").update(codeVerifier).digest());
      // Raycast's redirect page only passes a state on when it is JSON it recognises.
      const state = base64url(JSON.stringify({ flavor: "release", id: crypto.randomUUID(), providerName: this.providerName }));
      const redirectURI = redirectFor(this.redirectMethod);
      return {
        codeVerifier, codeChallenge, state, redirectURI,
        toURL: () => {
          const url = new URL(options.endpoint);
          const set = (key, value) => value != null && url.searchParams.set(key, String(value));
          set("response_type", "code");
          set("client_id", options.clientId);
          set("redirect_uri", redirectURI);
          set("scope", options.scope);
          set("state", state);
          set("code_challenge", codeChallenge);
          set("code_challenge_method", "S256");
          for (const [key, value] of Object.entries(options.extraParameters || {})) set(key, value);
          return url.toString();
        },
      };
    }
    async authorize(options) {
      const url = typeof options.toURL === "function" ? options.toURL() : options.url;
      if (!url) throw new Error("OAuth: no authorization URL.");
      let state = options.state;
      try {
        state = state || new URL(url).searchParams.get("state") || "";
      } catch {}
      const go = await call("confirm", [{
        title: `Sign in to ${this.providerName}`,
        message: this.description || `${manifest.title || extensionName} needs your ${this.providerName} account. Your browser opens to sign in.`,
        confirm: "Sign In", destructive: false,
      }]);
      if (!go) throw new Error(`Sign-in to ${this.providerName} was cancelled.`);
      const answer = await call("oauth", [{ url, state, provider: this.providerName }]);
      return { authorizationCode: answer.code };
    }
    async setTokens(tokens) {
      const data = {
        accessToken: tokens.accessToken ?? tokens.access_token,
        refreshToken: tokens.refreshToken ?? tokens.refresh_token,
        idToken: tokens.idToken ?? tokens.id_token,
        expiresIn: tokens.expiresIn ?? tokens.expires_in,
        scope: tokens.scope,
        updatedAt: Date.now(),
      };
      fs.writeFileSync(this.tokensPath, JSON.stringify(data), { mode: 0o600 });
      fs.chmodSync(this.tokensPath, 0o600);
    }
    async getTokens() {
      try {
        const data = JSON.parse(fs.readFileSync(this.tokensPath, "utf8"));
        return data.accessToken ? new TokenSet(data) : undefined;
      } catch {
        return undefined;
      }
    }
    async removeTokens() {
      fs.rmSync(this.tokensPath, { force: true });
    }
  }

  // ---- System ------------------------------------------------------------------------------
  const open = async (target, application) => {
    const app = typeof application === "string" ? application : application?.path || application?.name;
    if (app && fs.existsSync(app) && app.endsWith(".desktop")) detached("gtk-launch", [path.basename(app), target]);
    else detached("xdg-open", [String(target)]);
  };
  const showInFinder = async (target) => {
    const uri = `file://${target}`;
    run("dbus-send", ["--session", "--print-reply", "--dest=org.freedesktop.FileManager1", "/org/freedesktop/FileManager1",
      "org.freedesktop.FileManager1.ShowItems", `array:string:${uri}`, "string:"]).catch(() => detached("xdg-open", [path.dirname(String(target))]));
  };
  const trash = async (targets) => {
    for (const target of [].concat(targets)) await run("gio", ["trash", String(target)]);
  };

  function desktopEntries() {
    const dirs = [path.join(dataHome, "applications"), "/usr/local/share/applications", "/usr/share/applications"];
    const apps = [];
    for (const dir of dirs) {
      let files = [];
      try {
        files = fs.readdirSync(dir);
      } catch {
        continue;
      }
      for (const file of files) {
        if (!file.endsWith(".desktop")) continue;
        try {
          const text = fs.readFileSync(path.join(dir, file), "utf8");
          const name = /^Name=(.*)$/m.exec(text)?.[1];
          if (!name || /^NoDisplay=true$/m.test(text)) continue;
          apps.push({ name, path: path.join(dir, file), bundleId: file.replace(/\.desktop$/, ""), localizedName: name });
        } catch {}
      }
    }
    return apps;
  }
  const getApplications = async () => desktopEntries();
  const getDefaultApplication = async (target) => {
    const mime = /^https?:/.test(String(target)) ? "x-scheme-handler/https" : (await run("xdg-mime", ["query", "filetype", String(target)])).trim();
    const id = (await run("xdg-mime", ["query", "default", mime])).trim();
    const found = desktopEntries().find((a) => path.basename(a.path) === id);
    if (!found) throw new Error(`No default application for ${target}`);
    return found;
  };
  const getFrontmostApplication = async () => {
    const info = launch.frontmost || {};
    return { name: info.name || "", path: info.path || "", bundleId: info.bundleId || "", localizedName: info.name || "" };
  };
  const getSelectedText = async () => {
    const text = await run("wl-paste", ["--primary", "--no-newline"]).catch(() => "");
    if (!text) throw new Error("No text is selected.");
    return text;
  };
  const unsupported = (name) => async () => {
    throw new Error(`${name} is not available on Linux.`);
  };

  const environment = {
    extensionName,
    commandName: command.name,
    commandMode: command.mode,
    assetsPath: path.join(extensionDir, "assets"),
    supportPath,
    raycastVersion: "1.104.0",
    ownerOrAuthorName: manifest.owner || manifest.author || "",
    launchType: "userInitiated",
    isDevelopment: false,
    appearance: launch.appearance || "dark",
    theme: launch.appearance || "dark",
    textSize: "medium",
    canAccess: () => false,
  };

  // ---- Components --------------------------------------------------------------------------
  const NavigationContext = React.createContext(ctx.navigation);
  const useNavigation = () => React.useContext(NavigationContext);

  const Action = host("Action");
  const typed = (kind) => {
    const Component = (props) => h("Action", { ...props, kind });
    Component.displayName = `Action.${kind}`;
    return Component;
  };
  Action.CopyToClipboard = typed("CopyToClipboard");
  Action.Paste = typed("Paste");
  Action.OpenInBrowser = typed("OpenInBrowser");
  Action.Open = typed("Open");
  Action.OpenWith = typed("Open");
  Action.ShowInFinder = typed("ShowInFinder");
  Action.SubmitForm = typed("SubmitForm");
  Action.Trash = typed("Trash");
  Action.ToggleQuickLook = typed("QuickLook");
  Action.CreateSnippet = typed("Unsupported");
  Action.CreateQuicklink = typed("Unsupported");
  Action.PickDate = typed("Unsupported");
  Action.Push = (props) => {
    const navigation = useNavigation();
    const { target, onPush, onPop, ...rest } = props;
    return h("Action", {
      ...rest, kind: "Push",
      onAction: () => {
        navigation.push(target, onPop);
        onPush?.();
      },
    });
  };
  Action.Style = enums.ActionStyle;

  const ActionPanel = host("ActionPanel");
  ActionPanel.Section = host("ActionPanel.Section");
  ActionPanel.Submenu = host("ActionPanel.Submenu");

  const Dropdown = (prefix) => {
    const Component = host(`${prefix}.Dropdown`);
    Component.Item = host("Dropdown.Item");
    Component.Section = host("Dropdown.Section");
    return Component;
  };

  const metadata = (prefix) => {
    const Metadata = host(`${prefix}.Metadata`);
    Metadata.Label = host("Metadata.Label");
    Metadata.Link = host("Metadata.Link");
    Metadata.Separator = host("Metadata.Separator");
    Metadata.TagList = host("Metadata.TagList");
    Metadata.TagList.Item = host("Metadata.TagList.Item");
    return Metadata;
  };

  const List = host("List", { actions: "single", searchBarAccessory: "single" });
  List.Item = host("List.Item", { actions: "single", detail: "single" });
  List.Item.Detail = host("List.Item.Detail", { metadata: "single" });
  List.Item.Detail.Metadata = metadata("List.Item.Detail");
  List.Section = host("List.Section");
  List.EmptyView = host("EmptyView", { actions: "single" });
  List.Dropdown = Dropdown("List");

  const Grid = host("Grid", { actions: "single", searchBarAccessory: "single" });
  Grid.Item = host("Grid.Item", { actions: "single" });
  Grid.Section = host("Grid.Section");
  Grid.EmptyView = host("EmptyView", { actions: "single" });
  Grid.Dropdown = Dropdown("Grid");
  Grid.Fit = enums.GridFit;
  Grid.Inset = enums.GridInset;
  Grid.ItemSize = enums.GridItemSize;
  Grid.AspectRatio = enums.GridAspectRatio;

  const Detail = host("Detail", { actions: "single", metadata: "single" });
  Detail.Metadata = metadata("Detail");

  const Form = host("Form", { actions: "single", searchBarAccessory: "single" });
  for (const name of ["TextField", "PasswordField", "TextArea", "Checkbox", "DatePicker", "TagPicker", "FilePicker", "Description", "Separator"]) {
    Form[name] = host(`Form.${name}`);
  }
  Form.Dropdown = Dropdown("Form");
  Form.TagPicker.Item = host("Dropdown.Item");
  Form.LinkAccessory = host("Form.LinkAccessory");
  Form.DatePicker.Type = enums.DatePickerType;

  const MenuBarExtra = () => null;
  MenuBarExtra.Item = () => null;
  MenuBarExtra.Section = () => null;
  MenuBarExtra.Submenu = () => null;
  MenuBarExtra.Separator = () => null;

  const key = (k, mods) => ({ modifiers: mods, key: k });
  const Keyboard = {
    Shortcut: {
      Common: {
        Copy: key("c", ["cmd", "shift"]), CopyDeeplink: key("c", ["cmd", "shift"]), CopyName: key(".", ["cmd", "shift"]),
        CopyPath: key(",", ["cmd", "shift"]), Duplicate: key("d", ["cmd"]), Edit: key("e", ["cmd"]),
        MoveDown: key("arrowDown", ["cmd", "shift"]), MoveUp: key("arrowUp", ["cmd", "shift"]), New: key("n", ["cmd"]),
        Open: key("o", ["cmd"]), OpenWith: key("o", ["cmd", "shift"]), Pin: key("p", ["cmd", "shift"]),
        Refresh: key("r", ["cmd"]), Remove: key("x", ["ctrl"]), RemoveAll: key("x", ["ctrl", "shift"]),
        ToggleQuickLook: key("y", ["cmd"]), Save: key("s", ["cmd"]),
      },
    },
  };

  const api = {
    ...enums.nestedEnums,
    Action, ActionPanel, List, Grid, Detail, Form, MenuBarExtra,
    Icon: enums.Icon, Color: { ...enums.Color, Dynamic: undefined }, Image: enums.nestedEnums.Image, Keyboard,
    LaunchType: enums.LaunchType, PopToRootType: enums.PopToRootType,
    Toast, Alert, Clipboard, LocalStorage, Cache, environment,
    showToast, showHUD, confirmAlert, open, showInFinder, trash,
    useNavigation,
    getPreferenceValues: preferences,
    openExtensionPreferences: async () => notify("openPreferences", [extensionName, null]),
    openCommandPreferences: async () => notify("openPreferences", [extensionName, command.name]),
    closeMainWindow: async (options) => notify("closeMainWindow", [options || {}]),
    popToRoot: async (options) => notify("popToRoot", [options || {}]),
    clearSearchBar: async () => notify("clearSearchBar", []),
    launchCommand: async (options) => notify("launchCommand", [options]),
    updateCommandMetadata: async () => {},
    getApplications, getDefaultApplication, getFrontmostApplication, getSelectedText,
    getSelectedFinderItems: unsupported("getSelectedFinderItems"),
    captureException: (error) => console.error(error),
    randomId: () => crypto.randomUUID(),
    AI: { ask: unsupported("AI"), Model: {}, Creativity: {} },
    BrowserExtension: { getContent: unsupported("BrowserExtension"), getTabs: unsupported("BrowserExtension") },
    WindowManagement: {
      getActiveWindow: unsupported("WindowManagement"), getWindowsOnActiveDesktop: unsupported("WindowManagement"),
      getDesktops: unsupported("WindowManagement"), setWindowBounds: unsupported("WindowManagement"), DesktopType: enums.nestedEnums.WindowManagement.DesktopType,
    },
    OAuth: { ...enums.nestedEnums.OAuth, PKCEClient },
    __settle: async () => {
      while (inFlight.size) await Promise.allSettled([...inFlight]);
    },
  };
  // Extensions written before a newer API still import names this shim lacks; a missing
  // component should fail where it is used, with its name, not as "undefined is not a function".
  return new Proxy(api, {
    get(target, name) {
      if (name in target || typeof name === "symbol" || name === "__esModule" || name === "default") return target[name];
      if (name === "then") return undefined;
      return undefined;
    },
  });
}

export { execFileSync };
