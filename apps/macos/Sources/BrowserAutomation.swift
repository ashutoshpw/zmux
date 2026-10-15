import Foundation
import WebKit

// Original MIT browser host protocol. Pages receive no native message handler.
struct BrowserOwner: Codable, Sendable, Equatable {
    let epoch: UUID
    let identityNamespace: UUID
    let surfaceId: UInt64
    let hostId: UUID
    let documentGeneration: UInt64
    let modelNavigation: UInt64
}

struct BrowserDispatchBatch: Decodable, Sendable {
    let epoch: UUID
    let requests: [NativeBrowserRequest]
}

struct NativeBrowserRequest: Decodable, Sendable {
    let operationId: UInt64
    let owner: BrowserOwner
    let action: BrowserAction
    let timeoutMs: UInt64
}

struct BrowserSnapshotOptions: Codable, Sendable {
    var selector: String? = nil
    var interactive = false
    var cursor = false
    var compact = false
    var maxDepth: UInt32? = nil
}

struct BrowserLocator: Codable, Sendable {
    let kind: String
    let query: String?
    let name: String?
    let exact: Bool
    let selector: String?
    let index: UInt32?
}

enum BrowserZoom: Codable, Sendable {
    case `in`, out, reset, set(factor: Double)
    private enum CodingKeys: String, CodingKey { case kind, factor }
    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(String.self, forKey: .kind) {
        case "in": self = .in
        case "out": self = .out
        case "reset": self = .reset
        case "set": self = .set(factor: try c.decode(Double.self, forKey: .factor))
        default: throw DecodingError.dataCorruptedError(forKey: .kind, in: c, debugDescription: "Unsupported browser zoom")
        }
    }
    func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .in: try c.encode("in", forKey: .kind)
        case .out: try c.encode("out", forKey: .kind)
        case .reset: try c.encode("reset", forKey: .kind)
        case .set(let factor): try c.encode("set", forKey: .kind); try c.encode(factor, forKey: .factor)
        }
    }
}

enum BrowserAction: Codable, Sendable {
    case back(snapshotAfter: Bool)
    case forward(snapshotAfter: Bool)
    case reload(snapshotAfter: Bool)
    case zoom(operation: BrowserZoom)
    case eval(script: String)
    case snapshot(options: BrowserSnapshotOptions)
    case get(kind: String, selector: String?, attribute: String?, property: String?)
    case find(locator: BrowserLocator)
    case wait(selector: String, state: String)

    private enum CodingKeys: String, CodingKey { case action, snapshotAfter, script, options, kind, selector, attribute, property, locator, state, operation }
    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(String.self, forKey: .action) {
        case "back": self = .back(snapshotAfter: try c.decodeIfPresent(Bool.self, forKey: .snapshotAfter) ?? false)
        case "forward": self = .forward(snapshotAfter: try c.decodeIfPresent(Bool.self, forKey: .snapshotAfter) ?? false)
        case "reload": self = .reload(snapshotAfter: try c.decodeIfPresent(Bool.self, forKey: .snapshotAfter) ?? false)
        case "zoom": self = .zoom(operation: try c.decode(BrowserZoom.self, forKey: .operation))
        case "eval": self = .eval(script: try c.decode(String.self, forKey: .script))
        case "snapshot": self = .snapshot(options: try c.decodeIfPresent(BrowserSnapshotOptions.self, forKey: .options) ?? .init())
        case "find": self = .find(locator: try c.decode(BrowserLocator.self, forKey: .locator))
        case "wait": self = .wait(selector: try c.decode(String.self, forKey: .selector), state: try c.decode(String.self, forKey: .state))
        case "get": self = .get(kind: try c.decode(String.self, forKey: .kind), selector: try c.decodeIfPresent(String.self, forKey: .selector), attribute: try c.decodeIfPresent(String.self, forKey: .attribute), property: try c.decodeIfPresent(String.self, forKey: .property))
        default: throw DecodingError.dataCorruptedError(forKey: .action, in: c, debugDescription: "Unsupported browser action")
        }
    }
    func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .back(let after), .forward(let after), .reload(let after):
            let name: String
            switch self { case .back: name = "back"; case .forward: name = "forward"; default: name = "reload" }
            try c.encode(name, forKey: .action); try c.encode(after, forKey: .snapshotAfter)
        case .eval(let script): try c.encode("eval", forKey: .action); try c.encode(script, forKey: .script)
        case .zoom(let operation): try c.encode("zoom", forKey: .action); try c.encode(operation, forKey: .operation)
        case .snapshot(let options): try c.encode("snapshot", forKey: .action); try c.encode(options, forKey: .options)
        case .find(let locator): try c.encode("find", forKey: .action); try c.encode(locator, forKey: .locator)
        case .wait(let selector, let state):
            try c.encode("wait", forKey: .action); try c.encode(selector, forKey: .selector); try c.encode(state, forKey: .state)
        case .get(let kind, let selector, let attribute, let property):
            try c.encode("get", forKey: .action); try c.encode(kind, forKey: .kind)
            try c.encodeIfPresent(selector, forKey: .selector); try c.encodeIfPresent(attribute, forKey: .attribute); try c.encodeIfPresent(property, forKey: .property)
        }
    }
    var isNavigation: Bool { switch self { case .back, .forward, .reload: true; default: false } }
    var snapshotAfter: Bool { switch self { case .back(let after), .forward(let after), .reload(let after): after; default: false } }
    var outcomeMayBeUnknown: Bool { switch self { case .back, .forward, .reload, .eval, .zoom: true; default: false } }
    var isWait: Bool { if case .wait = self { return true }; return false }
}

indirect enum BrowserJSON: Codable, Sendable {
    case null, bool(Bool), unsigned(UInt64), number(Double), string(String), array([BrowserJSON]), object([String: BrowserJSON])
    init(from decoder: any Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let value = try? c.decode(Bool.self) { self = .bool(value) }
        else if let value = try? c.decode(UInt64.self) { self = .unsigned(value) }
        else if let value = try? c.decode(Double.self) { self = .number(value) }
        else if let value = try? c.decode(String.self) { self = .string(value) }
        else if let value = try? c.decode([BrowserJSON].self) { self = .array(value) }
        else { self = .object(try c.decode([String: BrowserJSON].self)) }
    }
    func encode(to encoder: any Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case .bool(let v): try c.encode(v)
        case .unsigned(let v): try c.encode(v)
        case .number(let v): try c.encode(v)
        case .string(let v): try c.encode(v)
        case .array(let v): try c.encode(v)
        case .object(let v): try c.encode(v)
        }
    }
}

struct BrowserAutomationFailure: Codable, Sendable, Error {
    let code: String
    let message: String
    let outcomeUnknown: Bool
}

enum BrowserHostEvent: Encodable, Sendable {
    case attach(owner: BrowserOwner)
    case detach(owner: BrowserOwner)
    case documentChanged(owner: BrowserOwner, documentGeneration: UInt64, modelNavigation: UInt64, causedBy: UInt64?)
    case observed(owner: BrowserOwner, url: String, title: String)
    case complete(operationId: UInt64, owner: BrowserOwner, result: BrowserJSON)
    case fail(operationId: UInt64, owner: BrowserOwner, error: BrowserAutomationFailure)

    private enum CodingKeys: String, CodingKey { case event, owner, epoch, surfaceId, hostId, documentGeneration, modelNavigation, causedBy, operationId, result, error, url, title }
    func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .attach(let owner):
            try c.encode("attach", forKey: .event); try c.encode(owner.epoch, forKey: .epoch)
            try c.encode(owner.surfaceId, forKey: .surfaceId); try c.encode(owner.hostId, forKey: .hostId)
            try c.encode(owner.documentGeneration, forKey: .documentGeneration); try c.encode(owner.modelNavigation, forKey: .modelNavigation)
        case .detach(let owner): try c.encode("detach", forKey: .event); try c.encode(owner, forKey: .owner)
        case .documentChanged(let owner, let generation, let navigation, let cause):
            try c.encode("document_changed", forKey: .event); try c.encode(owner, forKey: .owner)
            try c.encode(generation, forKey: .documentGeneration); try c.encode(navigation, forKey: .modelNavigation)
            try c.encodeIfPresent(cause, forKey: .causedBy)
        case .observed(let owner, let url, let title):
            try c.encode("observed", forKey: .event); try c.encode(owner, forKey: .owner)
            try c.encode(url, forKey: .url); try c.encode(title, forKey: .title)
        case .complete(let id, let owner, let result):
            try c.encode("complete", forKey: .event); try c.encode(id, forKey: .operationId)
            try c.encode(owner, forKey: .owner); try c.encode(result, forKey: .result)
        case .fail(let id, let owner, let error):
            try c.encode("fail", forKey: .event); try c.encode(id, forKey: .operationId)
            try c.encode(owner, forKey: .owner); try c.encode(error, forKey: .error)
        }
    }
}

/// The wrapper runs in an isolated WKContentWorld except for explicit user eval.
/// Every JS result is a bounded JSON string, never a bridged page object.
@MainActor
enum BrowserAutomationScript {
    static let isolatedWorld = WKContentWorld.world(name: "app.w3dev.zmux.automation")
    static let resultBytes = 60 * 1024
    // This fixed probe receives no user script. A policy decision is made before
    // user code starts, so a script's own CSP-looking exception cannot cause retry.
    static let evalPreflight = #"""
    try {
      return (0, eval)('0') === 0 ? 'allowed' : 'unsupported';
    } catch (error) {
      const message = typeof error.message === 'string' ? error.message.toLowerCase() : '';
      const policy = message.includes('unsafe-eval') && (message.includes('content security policy') || message.includes('csp') || message.includes('refused'));
      return error.name === 'EvalError' && policy ? 'csp_blocked' : 'unsupported';
    }
    """#

    static func isNativeCSPDenial(_ error: any Error) -> Bool {
        let native = error as NSError
        guard native.domain == WKError.errorDomain,
              native.code == WKError.Code.javaScriptExceptionOccurred.rawValue,
              let message = native.userInfo["WKJavaScriptExceptionMessage"] as? String else { return false }
        let value = message.lowercased()
        return value.contains("unsafe-eval") && (value.contains("content security policy") || value.contains("csp") || value.contains("refused"))
    }
    static let source = #"""
    const input = JSON.parse(payload);
    const req = input.action;
    let truncated = false;
    const fault = (code, message) => { const e = new Error(message); e.zmuxCode = code; throw e; };
    const text = (value, limit = 16000) => {
      const s = String(value ?? '');
      if (s.length > limit) { truncated = true; return s.slice(0, limit); }
      return s;
    };
    const prefix = '@zmux:' + input.host_id + ':' + input.document_generation + ':';
    let state = globalThis.__zmuxDocumentReferences;
    if (!state || state.prefix !== prefix) {
      state = {prefix, next: 1, nodes: new Map(), ids: new WeakMap()};
      globalThis.__zmuxDocumentReferences = state;
    }
    const resolve = selector => {
      if (selector?.startsWith('@')) {
        if (!selector.startsWith(prefix)) fault('stale_reference', 'Element reference belongs to another browser document');
        const node = state.nodes.get(selector);
        if (!node || !node.isConnected) fault('stale_reference', 'Referenced element no longer exists');
        return node;
      }
      try { return selector ? document.querySelector(selector) : document.body ?? document.documentElement; }
      catch (_) { fault('invalid_selector', 'Invalid CSS selector'); }
    };
    const reference = node => {
      let ref = state.ids.get(node);
      if (!ref) {
        if (state.nodes.size >= 4096) fault('reference_limit', 'Document element reference limit reached; reload to reset');
        ref = prefix + state.next++; state.nodes.set(ref, node); state.ids.set(node, ref);
      }
      return ref;
    };
    const visible = node => {
      const style = getComputedStyle(node), box = node.getBoundingClientRect();
      return style.display !== 'none' && style.visibility !== 'hidden' && style.visibility !== 'collapse' && box.width > 0 && box.height > 0;
    };
    const normalized = value => String(value ?? '').replace(/[\p{White_Space}\uFEFF]+/gu, ' ').trim();
    const matchesText = (value, query, exact) => exact ? normalized(value) === normalized(query) : normalized(value).toLowerCase().includes(normalized(query).toLowerCase());
    const elements = () => {
      if (!document.documentElement) return [];
      const walker = document.createTreeWalker(document.documentElement, NodeFilter.SHOW_ALL), nodes = [];
      let node = document.documentElement, visited = 0, characters = 0;
      while (node) {
        if (++visited > 16384) fault('dom_scan_limit', 'Locator enumeration exceeds 16384 DOM nodes');
        if (node.nodeType === 3) characters += node.length;
        if (characters > 1048576) fault('dom_scan_limit', 'Locator enumeration exceeds its text budget');
        if (node.nodeType === 1) {
          if (nodes.length === 4096) fault('dom_scan_limit', 'Locator enumeration exceeds 4096 elements');
          nodes.push(node);
        }
        node = walker.nextNode();
      }
      return nodes;
    };
    const matching = selector => {
      if (selector.startsWith('@')) return [resolve(selector)];
      elements(); // Bound document enumeration before allocating a CSS result set.
      try { return Array.from(document.querySelectorAll(selector)); }
      catch (_) { fault('invalid_selector', 'Invalid CSS selector'); }
    };
    const role = node => {
      const explicit = normalized(node.getAttribute('role'));
      if (explicit) return explicit.split(' ')[0].toLowerCase();
      if (/^H[1-6]$/.test(node.tagName)) return 'heading';
      if (node.tagName === 'A') return node.hasAttribute('href') ? 'link' : '';
      if (node.tagName === 'INPUT') {
        const type = node.type.toLowerCase();
        if (type === 'hidden') return '';
        return ({text:'textbox',email:'textbox',tel:'textbox',url:'textbox',checkbox:'checkbox',radio:'radio',range:'slider',number:'spinbutton',search:'searchbox',button:'button',submit:'button',reset:'button',image:'button'})[type] ?? '';
      }
      if (node.tagName === 'SELECT') return node.multiple || node.size > 1 ? 'listbox' : 'combobox';
      return ({BUTTON:'button',TEXTAREA:'textbox',IMG:'image',UL:'list',OL:'list',LI:'listitem',OPTION:'option',PROGRESS:'progressbar'})[node.tagName] ?? '';
    };
    const labels = node => {
      const ids = normalized(node.getAttribute('aria-labelledby'));
      const labelled = ids ? normalized(ids.split(' ').map(id => document.getElementById(id)?.textContent ?? '').join(' ')) : '';
      return labelled || normalized(node.getAttribute('aria-label')) || normalized(Array.from(node.labels ?? []).map(label => label.textContent).join(' '));
    };
    const accessibleName = node => {
      const label = labels(node);
      if (label) return label;
      if (node.tagName === 'IMG' || (node.tagName === 'INPUT' && node.type === 'image')) return normalized(node.getAttribute('alt')) || normalized(node.getAttribute('title'));
      if (node.tagName === 'INPUT' && ['button','submit','reset'].includes(node.type) && node.value) return normalized(node.value);
      if (['button','link'].includes(role(node))) return normalized(node.textContent) || normalized(node.getAttribute('title'));
      return normalized(node.getAttribute('title'));
    };
    const enabled = node => !node.matches(':disabled') && node.getAttribute('aria-disabled') !== 'true';
    const checked = node => Boolean(node.checked) || node.getAttribute('aria-checked') === 'true';
    const descriptor = node => ({ref:reference(node), role:text(role(node),256), name:text(accessibleName(node),256), text:text(normalized(node.textContent),384)});
    const find = locator => {
      let nodes;
      if (['first','last','nth'].includes(locator.kind)) {
        nodes = matching(locator.selector);
        const index = locator.kind === 'first' ? 0 : locator.kind === 'last' ? nodes.length - 1 : locator.index;
        if (!nodes[index]) fault('element_not_found', 'Locator index did not match an element');
        return descriptor(nodes[index]);
      }
      nodes = elements().filter(node => !node.closest('script,style,noscript,template'));
      // Build text once from the bounded enumeration. Excluded subtrees cannot
      // reappear as an ancestor match merely because textContent includes them.
      const contents = new Map();
      if (locator.kind === 'text') {
        for (let index = nodes.length - 1; index >= 0; index--) {
          const node = nodes[index];
          contents.set(node, Array.from(node.childNodes, child => child.nodeType === 3 ? child.data : contents.get(child) ?? '').join(''));
        }
      }
      const query = locator.query;
      nodes = nodes.filter(node => {
        switch (locator.kind) {
        case 'role': return role(node) === query.toLowerCase() && (locator.name == null || matchesText(accessibleName(node), locator.name, locator.exact));
        case 'text': return matchesText(contents.get(node), query, locator.exact);
        case 'label': return labels(node) !== '' && matchesText(labels(node), query, locator.exact);
        case 'placeholder': case 'alt': case 'title': case 'testid': {
          const attribute = locator.kind === 'testid' ? 'data-testid' : locator.kind;
          return node.hasAttribute(attribute) && matchesText(node.getAttribute(attribute), query, locator.exact);
        }
        default: fault('invalid_argument', 'Unknown semantic locator');
        }
      });
      if (locator.kind === 'text') {
        const parents = new Set();
        for (const node of nodes) { let parent = node.parentElement; while (parent) { parents.add(parent); parent = parent.parentElement; } }
        nodes = nodes.filter(node => !parents.has(node));
      }
      if (!nodes.length) fault('element_not_found', 'Locator did not match an element');
      if (nodes.length !== 1) fault('ambiguous_locator', 'Locator matched multiple elements; use first/last/nth explicitly');
      return descriptor(nodes[0]);
    };
    const wait = () => {
      const nodes = matching(req.selector);
      if (nodes.length > 1) fault('ambiguous_locator', 'Wait selector matched multiple elements');
      const node = nodes[0] ?? null;
      let matched;
      switch (req.state) {
      case 'attached': matched = node !== null; break;
      case 'detached': matched = node === null; break;
      case 'visible': matched = node !== null && visible(node); break;
      case 'hidden': matched = node === null || !visible(node); break;
      case 'enabled': matched = node !== null && enabled(node); break;
      case 'disabled': matched = node !== null && !enabled(node); break;
      case 'checked': matched = node !== null && checked(node); break;
      case 'unchecked': matched = node !== null && !checked(node); break;
      default: fault('invalid_argument', 'Unknown wait state');
      }
      return {matched,state:req.state,ref:matched && node ? reference(node) : null};
    };
    const snapshot = options => {
      const root = resolve(options.selector);
      if (!root) fault('element_not_found', 'Snapshot root was not found');
      const nodes = [], queue = [[root, 0]], maxDepth = Math.min(options.max_depth ?? 12, 32);
      let remaining = 46000;
      let visited = 0;
      while (queue.length && visited < 2048 && nodes.length < 192) {
        const [node, depth] = queue.shift(); visited++;
        if (node.nodeType !== 1 || ['SCRIPT','STYLE','NOSCRIPT','TEMPLATE'].includes(node.tagName)) continue;
        const interactive = node.matches('a[href],button,input,select,textarea,summary,[role="button"],[role="link"],[tabindex]');
        const cursor = options.cursor && getComputedStyle(node).cursor === 'pointer';
        if (visible(node) && (!options.interactive || interactive || cursor)) {
          const role = node.getAttribute('role') || ({A:'link',BUTTON:'button',INPUT:'textbox',SELECT:'combobox',TEXTAREA:'textbox',IMG:'image'}[node.tagName] ?? node.tagName.toLowerCase());
          const name = node.getAttribute('aria-label') || node.getAttribute('alt') || node.getAttribute('title') || '';
          const direct = Array.from(node.childNodes).filter(n => n.nodeType === 3).map(n => n.textContent).join(' ').trim();
          if (!options.compact || interactive || name || direct) {
            const item = {ref: reference(node), role, name: text(name, 256), text: text(direct, 384), depth};
            if (cursor) item.cursor = 'pointer';
            const bytes = new TextEncoder().encode(JSON.stringify(item)).length;
            if (bytes > remaining) { truncated = true; break; }
            remaining -= bytes; nodes.push(item);
          }
        }
        if (depth < maxDepth) {
          for (const child of node.children) {
            if (queue.length >= 2048) { truncated = true; break; }
            queue.push([child, depth + 1]);
          }
        } else if (node.children.length) truncated = true;
      }
      if (queue.length) truncated = true;
      return {url: text(location.href, 4096), title: text(document.title, 1024), nodes};
    };
    const get = () => {
      if (req.kind === 'url') return text(location.href, 4096);
      if (req.kind === 'title') return text(document.title, 1024);
      if (req.kind === 'count') {
        if (req.selector?.startsWith('@')) return resolve(req.selector) ? 1 : 0;
        try { return req.selector ? document.querySelectorAll(req.selector).length : 1; }
        catch (_) { fault('invalid_selector', 'Invalid CSS selector'); }
      }
      const node = resolve(req.selector);
      if (['visible','enabled','checked'].includes(req.kind) && !node) return false;
      if (!node) fault('element_not_found', 'Element was not found');
      switch (req.kind) {
      case 'text': return text(node.innerText ?? node.textContent);
      case 'html': return text(node.outerHTML);
      case 'value': return text(node.value);
      case 'attr':
        if (!req.attribute) fault('invalid_argument', 'Attribute name is required');
        return node.hasAttribute(req.attribute) ? text(node.getAttribute(req.attribute)) : null;
      case 'box': { const b = node.getBoundingClientRect(); return {x:b.x,y:b.y,width:b.width,height:b.height}; }
      case 'styles': {
        const style = getComputedStyle(node);
        if (req.property) return text(style.getPropertyValue(req.property), 4096);
        const result = {}; for (let i = 0; i < Math.min(style.length, 192); i++) result[style[i]] = text(style.getPropertyValue(style[i]), 128);
        if (style.length > 192) truncated = true;
        return result;
      }
      case 'visible': return visible(node);
      case 'enabled': return enabled(node);
      case 'checked': return checked(node);
      default: fault('unsupported_action', 'Unsupported browser getter');
      }
    };
    let visited = 0;
    const normalize = (value, depth = 0, seen = new Set()) => {
      if (++visited > 1024 || depth > 12) fault('result_too_large', 'Evaluation result exceeds the structural limit');
      if (value === undefined) return {__cmux_t:'undefined',__cmux_v:null};
      if (value === null) return null;
      if (typeof value === 'string') return text(value);
      if (typeof value === 'boolean') return value;
      if (typeof value === 'number' && Number.isFinite(value)) return value;
      if (typeof value !== 'object' || seen.has(value)) fault('unsupported_result', 'Evaluation must return a finite JSON-compatible value');
      seen.add(value);
      let result;
      const tag = Object.prototype.toString.call(value);
      if (tag === '[object DOMRect]' || tag === '[object DOMRectReadOnly]') {
        result = {};
        for (const key of ['x','y','width','height','top','right','bottom','left']) result[key] = normalize(value[key], depth + 1, seen);
      } else if (Array.isArray(value)) {
        if (value.length > 512) fault('result_too_large', 'Evaluation array is too large');
        result = value.map(item => normalize(item, depth + 1, seen));
      } else {
        const keys = Object.keys(value);
        if (keys.length > 128) fault('result_too_large', 'Evaluation object is too large');
        result = Object.create(null); for (const key of keys) result[text(key, 256)] = normalize(value[key], depth + 1, seen);
      }
      seen.delete(value); return result;
    };
    try {
      let value;
      if (req.action === 'eval') value = normalize(await (0, eval)(req.script));
      else if (req.action === 'snapshot') value = snapshot(req.options ?? {});
      else if (req.action === 'get') value = get();
      else if (req.action === 'find') value = find(req.locator);
      else if (req.action === 'wait') value = wait();
      else fault('unsupported_action', 'Unsupported browser action');
      const result = JSON.stringify({value, truncated});
      if (new TextEncoder().encode(result).length > 58000) fault('result_too_large', 'Browser result exceeds the byte limit; narrow the selector');
      return result;
    } catch (error) {
      return JSON.stringify({error:{code:String(error.zmuxCode ?? 'javascript_failed'),message:String(error.message ?? error).slice(0, 1024)}});
    }
    """#
}
