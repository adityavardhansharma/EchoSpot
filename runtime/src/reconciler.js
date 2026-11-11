// A React host renderer whose "DOM" is a plain JSON tree EchoSpot draws natively.
//
// Two conventions carry the Raycast component surface:
//   * `__slot` instances fold their children into the parent's props under a name, so
//     element-valued props (`actions={<ActionPanel/>}`, `detail={<List.Item.Detail/>}`) arrive
//     as structure.
//   * function props become `{"$fn": "<nodeId>:<prop>"}` handles, dispatched back by id.

import Reconciler from "react-reconciler";
import { DefaultEventPriority } from "react-reconciler/constants";

export const SLOT = "__slot";

let nextId = 1;
const node = (type, props) => ({ id: nextId++, type, props: props ?? {}, children: [] });

function detach(parent, child) {
  const index = parent.children.indexOf(child);
  if (index >= 0) parent.children.splice(index, 1);
}
function insert(parent, child, before) {
  detach(parent, child);
  const index = parent.children.indexOf(before);
  if (index < 0) parent.children.push(child);
  else parent.children.splice(index, 0, child);
}

const config = {
  supportsMutation: true,
  supportsPersistence: false,
  supportsHydration: false,
  isPrimaryRenderer: true,
  noTimeout: -1,
  warnsIfNotActing: false,
  getRootHostContext: () => null,
  getChildHostContext: (context) => context,
  getPublicInstance: (instance) => instance,
  createInstance: (type, props) => node(type, props),
  createTextInstance: (text) => ({ id: nextId++, type: "#text", text, children: [] }),
  appendInitialChild: (parent, child) => parent.children.push(child),
  appendChild: (parent, child) => {
    detach(parent, child);
    parent.children.push(child);
  },
  appendChildToContainer: (container, child) => {
    detach(container, child);
    container.children.push(child);
  },
  insertBefore: insert,
  insertInContainerBefore: insert,
  removeChild: detach,
  removeChildFromContainer: detach,
  clearContainer: (container) => {
    container.children.length = 0;
  },
  finalizeInitialChildren: () => false,
  shouldSetTextContent: () => false,
  commitUpdate: (instance, type, previous, next) => {
    instance.props = next;
  },
  commitTextUpdate: (instance, previous, next) => {
    instance.text = next;
  },
  commitMount: () => {},
  resetTextContent: () => {},
  prepareForCommit: () => null,
  resetAfterCommit: (container) => container.onCommit(),
  preparePortalMount: () => {},
  detachDeletedInstance: () => {},
  scheduleTimeout: setTimeout,
  cancelTimeout: clearTimeout,
  scheduleMicrotask: queueMicrotask,
  supportsMicrotasks: true,
  getCurrentUpdatePriority: () => DefaultEventPriority,
  setCurrentUpdatePriority: () => {},
  resolveUpdatePriority: () => DefaultEventPriority,
  getCurrentEventPriority: () => DefaultEventPriority,
  shouldAttemptEagerTransition: () => false,
  requestPostPaintCallback: () => {},
  trackSchedulerEvent: () => {},
  resolveEventType: () => null,
  resolveEventTimeStamp: () => -1.1,
  maySuspendCommit: () => false,
  preloadInstance: () => true,
  startSuspendingCommit: () => {},
  suspendInstance: () => {},
  waitForCommitToBeReady: () => null,
  NotPendingTransition: null,
  HostTransitionContext: {
    $$typeof: Symbol.for("react.context"), Provider: null, Consumer: null,
    _currentValue: null, _currentValue2: null, _threadCount: 0,
  },
  resetFormInstance: () => {},
  bindToConsole: (method, args) => () => method.apply(console, args),
  beforeActiveInstanceBlur: () => {},
  afterActiveInstanceBlur: () => {},
  prepareScopeUpdate: () => {},
  getInstanceFromScope: () => null,
  getInstanceFromNode: () => null,
};

const reconciler = Reconciler(config);

/// A fragment in a single slot flattens into same-typed siblings; merge them rather than keep
/// only the first.
function mergeSingle(contents) {
  if (contents.length === 1) return contents[0];
  const [first, ...rest] = contents;
  if (rest.some((n) => n.type !== first.type)) return first;
  const merged = { id: first.id, type: first.type, props: { ...first.props }, children: [...first.children] };
  for (const n of rest) {
    Object.assign(merged.props, n.props);
    merged.children.push(...n.children);
  }
  return merged;
}

export class Surface {
  constructor(onTree, onError) {
    this.handlers = new Map();
    this.onTree = onTree;
    this.onError = onError;
    this.container = { id: 0, type: "#root", props: {}, children: [], onCommit: () => this.flush() };
    this.root = reconciler.createContainer(
      this.container, 0, null, false, null, "echospot",
      (error) => this.onError(error), (error) => this.onError(error), (error) => this.onError(error), null,
    );
  }

  render(element) {
    reconciler.updateContainer(element, this.root, null, null);
  }

  unmount() {
    try {
      reconciler.updateContainer(null, this.root, null, null);
    } catch (error) {
      this.onError(error);
    }
    this.handlers.clear();
  }

  flush() {
    this.handlers.clear();
    const children = this.container.children.map((child) => this.serialize(child)).filter(Boolean);
    this.onTree({ children });
  }

  dispatch(handlerId, args) {
    const handler = this.handlers.get(handlerId);
    if (!handler) return false;
    try {
      Promise.resolve(handler(...args)).catch((error) => this.onError(error));
    } catch (error) {
      this.onError(error);
    }
    return true;
  }

  serialize(n) {
    if (n.type === "#text") return { type: "#text", text: String(n.text) };
    const props = {};
    for (const key of Object.keys(n.props)) {
      if (key === "children") continue;
      const value = this.encode(n.props[key], n.id, key);
      if (value !== undefined) props[key] = value;
    }
    const children = [];
    for (const child of n.children) {
      if (child.type === SLOT) {
        const name = child.props?.name;
        if (!name) continue;
        const contents = child.children.map((entry) => this.serialize(entry)).filter(Boolean);
        if (contents.length) props[name] = child.props.single ? mergeSingle(contents) : contents;
        continue;
      }
      const serialized = this.serialize(child);
      if (serialized) children.push(serialized);
    }
    return { id: n.id, type: n.type, props, children };
  }

  encode(value, nodeId, key) {
    if (value === undefined) return undefined;
    if (value === null) return null;
    switch (typeof value) {
      case "function": {
        const id = `${nodeId}:${key}`;
        this.handlers.set(id, value);
        return { $fn: id };
      }
      case "string":
      case "number":
      case "boolean":
        return value;
      case "bigint":
        return Number(value);
      case "symbol":
        return undefined;
    }
    if (value instanceof Date) return { $date: value.toISOString() };
    if (Array.isArray(value)) return value.map((item, i) => this.encode(item, nodeId, `${key}.${i}`) ?? null);
    if (value.$$typeof) return undefined;
    const out = {};
    for (const name of Object.keys(value)) {
      const encoded = this.encode(value[name], nodeId, `${key}.${name}`);
      if (encoded !== undefined) out[name] = encoded;
    }
    return out;
  }
}
