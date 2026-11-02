// Invocation-scoped OpenCode v2 plugin. It observes the exact root session and
// never changes prompts, permissions, questions, tools, or provider settings.
import * as fs from "node:fs";
import * as net from "node:net";
import { execFileSync } from "node:child_process";

const EVENT_LIMIT = 4096;
const QUEUE_LIMIT = 64;
const SNAPSHOT_MS = 5000;
const SESSION_CACHE_LIMIT = 128;
const WAIT_LIMIT = 128;
const fifoAtomicLimits = new Map();

function fifoAtomicLimit(path, metadata) {
  const key = `${metadata.dev}:${metadata.ino}`;
  if (fifoAtomicLimits.has(key)) return fifoAtomicLimits.get(key);
  // Node does not expose fpathconf. Probe once per FIFO; 512 bytes is the
  // POSIX guarantee when getconf is absent or cannot establish a larger bound.
  let limit = 512;
  try {
    const output = execFileSync("getconf", ["PIPE_BUF", path], {
      timeout: 500, maxBuffer: 128, stdio: ["ignore", "pipe", "ignore"],
    }).toString("ascii").trim();
    const reported = Number(output);
    if (/^[0-9]+$/u.test(output) && Number.isSafeInteger(reported) && reported >= 512) {
      limit = Math.min(reported, EVENT_LIMIT);
    }
  } catch {}
  if (fifoAtomicLimits.size >= 4) fifoAtomicLimits.clear();
  fifoAtomicLimits.set(key, limit);
  return limit;
}

function validID(value) {
  return typeof value === "string" && value.length > 0 &&
    Buffer.byteLength(value, "utf8") <= 256 && !/[\u0000-\u001f\u007f]/u.test(value);
}

function sanitizeObservation(sessionID, turnID, state, transition, marker) {
  if (!validID(sessionID) || (turnID !== null && !validID(turnID))) return null;
  if ((state === "working" || state === "waiting") && turnID === null) return null;
  return {
    provider: "opencode",
    scope: "root",
    session_id: sessionID,
    turn_id: turnID,
    state,
    transition,
    marker,
  };
}

function localSocketTransport(path) {
  return {
    captureGeneration() { return null; },
    async write(line) {
      await new Promise((resolve, reject) => {
        const socket = net.createConnection(path);
        socket.setNoDelay(true);
        socket.setTimeout(500, () => socket.destroy(new Error("provider event route timed out")));
        socket.once("error", reject);
        socket.once("connect", () => socket.end(line, () => {
          socket.destroy();
          resolve();
        }));
      });
      return null;
    },
    close() {},
    lost() {
      const socket = net.createConnection(path);
      socket.setTimeout(500, () => socket.destroy(new Error("provider loss route timed out")));
      socket.once("error", () => {});
      socket.once("connect", () => socket.end('{"kind":"provider_loss"}\n'));
    },
  };
}

function readRemoteGeneration() {
  const root = process.env.ZMUX_SHELL_INTEGRATION_DIRECTORY;
  if (!root) return null;
  let descriptor;
  try {
    descriptor = fs.openSync(root + "/agent-lifecycle-generation",
      fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW || 0) | (fs.constants.O_CLOEXEC || 0));
    const metadata = fs.fstatSync(descriptor);
    if (!metadata.isFile() || metadata.uid !== process.getuid() ||
        (metadata.mode & 0o777) !== 0o600 || metadata.nlink !== 1) return null;
    const value = fs.readFileSync(descriptor, "utf8").trim();
    return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u.test(value)
      ? value : null;
  } catch {
    return null;
  } finally {
    if (descriptor !== undefined) fs.closeSync(descriptor);
  }
}

function writeRemoteFIFO(path, frame) {
  const root = process.env.ZMUX_SHELL_INTEGRATION_DIRECTORY;
  const nonce = process.env.ZMUX_AGENT_LIFECYCLE_NONCE;
  if (!root || !/^[0-9a-f]{32}$/u.test(nonce || "")) throw new Error("remote provider route unavailable");
  const bytes = Buffer.from(JSON.stringify({ nonce, ...frame }) + "\n", "utf8");
  if (bytes.length > EVENT_LIMIT) throw new Error("provider FIFO frame exceeded bound");
  const flags = fs.constants.O_WRONLY | fs.constants.O_NONBLOCK |
    (fs.constants.O_NOFOLLOW || 0) | (fs.constants.O_CLOEXEC || 0);
  const descriptor = fs.openSync(path, flags);
  try {
    const metadata = fs.fstatSync(descriptor);
    if (!metadata.isFIFO() || metadata.uid !== process.getuid() ||
        (metadata.mode & 0o777) !== 0o600 || metadata.nlink !== 1) {
      throw new Error("unsafe provider lifecycle FIFO");
    }
    if (bytes.length > fifoAtomicLimit(path, metadata)) {
      throw new Error("provider lifecycle frame exceeds the FIFO atomic write bound");
    }
    if (fs.writeSync(descriptor, bytes) !== bytes.length) throw new Error("short provider FIFO write");
  } finally {
    fs.closeSync(descriptor);
  }
}

async function waitForRemoteGeneration(timeoutMs = 5000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const generation = readRemoteGeneration();
    if (generation) return generation;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error("managed SSH lifecycle stream is not active");
}

function remoteFIFOTransport() {
  const root = process.env.ZMUX_SHELL_INTEGRATION_DIRECTORY;
  if (!root || !/^[0-9a-f]{32}$/u.test(process.env.ZMUX_AGENT_LIFECYCLE_NONCE || "")) {
    throw new Error("remote provider route unavailable");
  }
  const fifo = process.env.ZMUX_PROVIDER_EVENTS_FIFO || root + "/agent-events";
  const lossFIFO = process.env.ZMUX_PROVIDER_LOSS_FIFO || root + "/agent-events-loss";
  let lastGeneration = null;
  return {
    captureGeneration: readRemoteGeneration,
    lossOnClose: true,
    async write(line, capturedGeneration, currentSnapshot) {
      const generation = capturedGeneration || await waitForRemoteGeneration();
      if (readRemoteGeneration() !== generation) return null;
      if (lastGeneration !== generation) {
        const source = currentSnapshot() || JSON.parse(line).observation;
        const marker = lastGeneration === null ? "baseline" : "recovery";
        writeRemoteFIFO(fifo, {
          generation,
          provider_observation: { ...source, transition: "snapshot", marker },
        });
        lastGeneration = generation;
        return generation;
      }
      const observation = JSON.parse(line).observation;
      writeRemoteFIFO(fifo, { generation, provider_observation: observation });
      return null;
    },
    close() {},
    lost() {
      try { writeRemoteFIFO(lossFIFO, { generation: readRemoteGeneration() }); } catch {}
    },
  };
}

function createTransport() {
  const localSocket = process.env.ZMUX_PROVIDER_EVENT_SOCKET;
  if (localSocket) return localSocketTransport(localSocket);
  return remoteFIFOTransport();
}

const ZmuxProviderLifecycle = {
  id: "zmux.provider.lifecycle",

  async setup(context) {
    let transport;
    try {
      transport = createTransport();
    } catch {
      return;
    }

    const queue = [];
    const observedStatuses = new Map();
    const pendingWaits = new Map();
    const expectedSession = validID(process.env.ZMUX_OPENCODE_EXPECTED_SESSION_ID)
      ? process.env.ZMUX_OPENCODE_EXPECTED_SESSION_ID : null;
    let rootSession = null;
    let activeTurn = null;
    let currentState = null;
    let providerStatus = null;
    let idleCandidate = null;
    let succeededTurn = null;
    let baselinePending = true;
    let transportFailed = false;
    let connected = true;
    let draining = false;
    const abort = new AbortController();

    function loseEvidence() {
      connected = false;
      if (!transportFailed) {
        transportFailed = true;
        void transport.lost?.();
      }
    }

    function enqueue(observation) {
      if (!connected || !observation) return;
      const generation = transport.captureGeneration?.() ?? null;
      const line = JSON.stringify({ kind: "opencode_observation", observation }) + "\n";
      if (Buffer.byteLength(line, "utf8") > EVENT_LIMIT || queue.length >= QUEUE_LIMIT) {
        loseEvidence();
        return;
      }
      queue.push({ line, generation });
      void drain();
    }

    function currentSnapshot() {
      if (!rootSession || currentState === null) return null;
      return sanitizeObservation(rootSession, activeTurn, currentState, "snapshot", "live");
    }

    async function drain() {
      if (draining || !connected) return;
      draining = true;
      try {
        while (queue.length > 0 && connected) {
          const item = queue.shift();
          const attachedGeneration = await transport.write(
            item.line,
            item.generation,
            currentSnapshot,
          );
          if (attachedGeneration) {
            // The attachment snapshot represents all queued state from before
            // it. Old events cannot replay as live transitions afterward.
            queue.length = 0;
          }
        }
      } catch {
        loseEvidence();
      } finally {
        draining = false;
      }
    }

    function report(state, transition, marker = "live", turnID = activeTurn) {
      const observation = sanitizeObservation(rootSession, turnID, state, transition, marker);
      if (observation) enqueue(observation);
    }

    function setState(state) {
      if (!activeTurn) {
        currentState = state;
        return;
      }
      if (currentState === state) return;
      currentState = state;
      report(state, "state_changed");
    }

    function clearTurn() {
      activeTurn = null;
      idleCandidate = null;
      succeededTurn = null;
      pendingWaits.clear();
    }

    function confirmCompletion() {
      if (!activeTurn || idleCandidate !== activeTurn || succeededTurn !== activeTurn ||
          providerStatus !== "idle" || pendingWaits.size !== 0 || !connected) return;
      const completedTurn = activeTurn;
      currentState = "idle";
      report("idle", "turn_completed", "live", completedTurn);
      clearTurn();
    }

    function reportedState(status) {
      switch (status?.type) {
        case "busy":
        case "retry": return "working";
        case "idle": return "idle";
        default: throw new Error("OpenCode returned an unsupported session status");
      }
    }

    function rememberStatus(sessionID, status) {
      if (!validID(sessionID)) return;
      const state = reportedState(status);
      if (observedStatuses.has(sessionID)) observedStatuses.delete(sessionID);
      else if (observedStatuses.size >= SESSION_CACHE_LIMIT) {
        observedStatuses.delete(observedStatuses.keys().next().value);
      }
      observedStatuses.set(sessionID, state);
    }

    function observeStatus(state) {
      providerStatus = state;
      if (!activeTurn) {
        currentState = state;
        if (state === "idle" && baselinePending) {
          report("idle", "snapshot", "baseline", null);
          baselinePending = false;
        }
        return;
      }
      if (pendingWaits.size > 0) {
        setState("waiting");
        return;
      }
      if (state === "idle") {
        currentState = "idle";
        confirmCompletion();
        if (activeTurn) report("idle", "snapshot", "live", activeTurn);
        return;
      }
      setState("working");
    }

    async function bindRoot(sessionID, createdHere) {
      if (rootSession || !validID(sessionID) ||
          (expectedSession && sessionID !== expectedSession)) return;
      try {
        const result = await context.session.get({ sessionID });
        const info = result?.data ?? result;
        if (info?.id !== sessionID || (info.parentID !== undefined && info.parentID !== null)) return;
        rootSession = sessionID;
        const previousStatus = observedStatuses.get(sessionID);
        if (createdHere) {
          providerStatus = "idle";
          currentState = "idle";
          report("idle", "snapshot", "baseline", null);
          baselinePending = false;
        } else if (previousStatus) {
          observeStatus(previousStatus);
        }
      } catch {
        // An unverified or currently unavailable session remains untracked.
      }
    }

    function waitKey(type, properties) {
      const id = [properties.id, properties.requestID, properties.questionID, properties.permissionID]
        .find(validID);
      return id ? type + ":" + id : type + ":unkeyed";
    }

    function addWait(type, properties) {
      const key = waitKey(type, properties);
      if (!pendingWaits.has(key) && pendingWaits.size >= WAIT_LIMIT) {
        loseEvidence();
        return;
      }
      pendingWaits.set(key, (pendingWaits.get(key) ?? 0) + 1);
      setState("waiting");
    }

    function resolveWait(type, properties) {
      const exact = waitKey(type, properties);
      const key = pendingWaits.has(exact) ? exact : type + ":unkeyed";
      if (!pendingWaits.has(key)) return;
      const count = pendingWaits.get(key);
      if (count <= 1) pendingWaits.delete(key);
      else pendingWaits.set(key, count - 1);
      if (pendingWaits.size === 0 && currentState === "waiting" && providerStatus === "working") {
        setState("working");
      }
    }

    async function consume() {
      try {
        for await (const item of context.event.subscribe({ signal: abort.signal })) {
          if (!connected) return;
          const type = item?.type;
          const data = item?.data ?? {};
          if (type === "session.status") {
            const state = reportedState(data.status);
            rememberStatus(data.sessionID, data.status);
            if (rootSession && data.sessionID === rootSession) observeStatus(state);
            continue;
          }
          if (type === "session.created") {
            if (data.parentID === undefined || data.parentID === null) {
              await bindRoot(data.sessionID, true);
            }
            continue;
          }
          if (type === "session.viewed") {
            await bindRoot(data.sessionID, false);
            continue;
          }
          const eventSessionID = type === "form.created"
            ? data.form?.sessionID
            : data.sessionID;
          if (!rootSession || eventSessionID !== rootSession) continue;
          if (type === "permission.asked") {
            addWait("permission", data);
          } else if (type === "permission.replied") {
            resolveWait("permission", data);
          } else if (type === "form.created") {
            addWait("form", data.form ?? {});
          } else if (type === "form.replied" || type === "form.cancelled") {
            resolveWait("form", data);
          } else if (type === "session.execution.succeeded") {
            if (activeTurn) {
              succeededTurn = activeTurn;
              confirmCompletion();
            }
          } else if (type === "session.execution.failed" && activeTurn) {
            const failedTurn = activeTurn;
            currentState = "idle";
            report("idle", "turn_failed", "live", failedTurn);
            clearTurn();
          } else if (type === "session.execution.interrupted" && activeTurn) {
            const interruptedTurn = activeTurn;
            currentState = "idle";
            report("idle", "turn_interrupted", "live", interruptedTurn);
            clearTurn();
          } else if (type === "session.deleted") {
            loseEvidence();
          }
        }
      } catch {
        if (connected) loseEvidence();
      }
    }

    const promptRegistration = await context.session.hook("prompt", async (input) => {
      if (!rootSession || input.sessionID !== rootSession || !validID(input.messageID)) return;
      // A second prompt while the root run is still active cannot be correlated
      // to one exact turn with the session-level status stream.
      if (activeTurn || providerStatus !== "idle") {
        loseEvidence();
        return;
      }
      activeTurn = input.messageID;
      idleCandidate = null;
      succeededTurn = null;
      pendingWaits.clear();
      currentState = "idle";
    });

    const idleRegistration = await context.session.hook("experimental.idle", async (input) => {
      if (!rootSession || input.sessionID !== rootSession || !activeTurn || !connected) return;
      if (!Array.isArray(input.background) || input.background.length !== 0 || pendingWaits.size !== 0) return;
      idleCandidate = activeTurn;
      confirmCompletion();
    });

    const consumer = consume();
    const timer = setInterval(() => {
      if (currentState !== null) report(currentState, "snapshot", "live", activeTurn);
    }, SNAPSHOT_MS);

    return async () => {
      if (transport.lossOnClose && rootSession && !transportFailed) {
        transport.lost();
      }
      connected = false;
      clearInterval(timer);
      abort.abort();
      await promptRegistration.dispose();
      await idleRegistration.dispose();
      transport.close();
      await consumer.catch(() => {});
    };
  },
};

export default ZmuxProviderLifecycle;
