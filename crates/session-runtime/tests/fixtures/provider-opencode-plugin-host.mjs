import * as net from "node:net";
import * as readline from "node:readline";
import { pathToFileURL } from "node:url";

const [pluginPath, socketPath, rootSession, routeMode = "capture"] = process.argv.slice(2);
const captureRoute = routeMode === "capture";
const outputs = [];
let outputPending = "";
let server;
if (captureRoute) {
  server = net.createServer((socket) => {
    socket.on("data", (bytes) => {
      outputPending += bytes.toString("utf8");
      while (outputPending.includes("\n")) {
        const newline = outputPending.indexOf("\n");
        const line = outputPending.slice(0, newline);
        outputPending = outputPending.slice(newline + 1);
        if (!line) continue;
        const value = JSON.parse(line);
        outputs.push(value);
        process.stdout.write(JSON.stringify(value) + "\n");
      }
    });
  });

  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(socketPath, resolve);
  });
}

const events = [];
const eventWaiters = [];
function pushEvent(value) {
  const waiter = eventWaiters.shift();
  if (waiter) waiter(value);
  else events.push(value);
}

const hooks = new Map();
const context = {
  event: {
    subscribe({ signal }) {
      return {
        [Symbol.asyncIterator]() { return this; },
        next() {
          if (events.length > 0) return Promise.resolve({ value: events.shift(), done: false });
          if (signal.aborted) return Promise.resolve({ done: true });
          return new Promise((resolve) => {
            const abort = () => resolve({ done: true });
            signal.addEventListener("abort", abort, { once: true });
            eventWaiters.push((value) => {
              signal.removeEventListener("abort", abort);
              resolve({ value, done: false });
            });
          });
        },
      };
    },
  },
  session: {
    async get({ sessionID }) {
      return { id: sessionID, parentID: null };
    },
    async hook(name, callback) {
      hooks.set(name, callback);
      return { async dispose() {} };
    },
  },
};

const plugin = (await import(pathToFileURL(pluginPath).href)).default;
const cleanup = await plugin.setup(context);
process.stdout.write('{"ready":true}\n');

async function waitForOutput(count) {
  if (!captureRoute) return;
  const deadline = Date.now() + 2000;
  while (outputs.length < count && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  if (outputs.length < count) throw new Error("provider plugin did not emit the expected observation");
}

async function waitForTransition(transition) {
  if (!captureRoute) return;
  const deadline = Date.now() + 3000;
  while (Date.now() < deadline) {
    const match = outputs.find((value) => value.observation.transition === transition && !value.seen);
    if (match) {
      match.seen = true;
      return match;
    }
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  throw new Error("provider plugin did not emit " + transition);
}

const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of input) {
  const command = JSON.parse(line);
  switch (command.op) {
    case "created":
      pushEvent({
        type: "session.created",
        data: { sessionID: "session:foreign/root", parentID: rootSession },
      });
      pushEvent({
        type: "session.created",
        data: { sessionID: rootSession, parentID: null },
      });
      await waitForOutput(1);
      if (captureRoute) outputs[0].seen = true;
      break;
    case "foreign":
      pushEvent({
        type: "permission.asked",
        data: { sessionID: "session:foreign/root", id: "foreign/request" },
      });
      await new Promise((resolve) => setTimeout(resolve, 30));
      break;
    case "start":
      await hooks.get("prompt")({
        sessionID: rootSession,
        messageID: "turn/opaque:1",
        prompt: "private prompt data must not be forwarded",
      });
      pushEvent({
        type: "session.status",
        data: { sessionID: rootSession, status: { type: "busy" } },
      });
      await waitForTransition("state_changed");
      break;
    case "wait":
      pushEvent({
        type: "permission.asked",
        data: { sessionID: rootSession, id: "permission/opaque:1" },
      });
      await waitForTransition("state_changed");
      break;
    case "resume":
      pushEvent({
        type: "permission.replied",
        data: { sessionID: rootSession, id: "permission/opaque:1" },
      });
      await waitForTransition("state_changed");
      break;
    case "form-wait":
      pushEvent({
        type: "form.created",
        data: { form: { id: "form/opaque:1", sessionID: rootSession } },
      });
      await waitForTransition("state_changed");
      break;
    case "form-resume":
      pushEvent({
        type: "form.replied",
        data: { id: "form/opaque:1", sessionID: rootSession, answer: {} },
      });
      await waitForTransition("state_changed");
      break;
    case "idle":
      pushEvent({
        type: "session.status",
        data: { sessionID: rootSession, status: { type: "idle" } },
      });
      pushEvent({
        type: "session.execution.succeeded",
        data: { sessionID: rootSession },
      });
      await waitForTransition("snapshot");
      break;
    case "complete":
      await hooks.get("experimental.idle")({ sessionID: rootSession, background: [] });
      await waitForTransition("turn_completed");
      break;
    case "next-start":
      await hooks.get("prompt")({
        sessionID: rootSession,
        messageID: "turn/opaque:2",
        prompt: "another private prompt",
      });
      pushEvent({
        type: "session.status",
        data: { sessionID: rootSession, status: { type: "retry", attempt: 1, message: "retry", next: 1 } },
      });
      await waitForTransition("state_changed");
      break;
    case "interrupt":
      pushEvent({
        type: "session.execution.interrupted",
        data: { sessionID: rootSession, reason: "user" },
      });
      await waitForTransition("turn_interrupted");
      break;
    case "background":
      await hooks.get("experimental.idle")({
        sessionID: rootSession,
        background: [{ kind: "shell", sessionID: rootSession, shellID: "opaque", command: "private" }],
      });
      await new Promise((resolve) => setTimeout(resolve, 30));
      break;
    case "shutdown":
      await cleanup();
      server?.close();
      process.exit(0);
      break;
    default:
      throw new Error("unknown fixture command");
  }
}
