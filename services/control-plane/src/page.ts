export const APP_CSS = String.raw`
:root { color-scheme: dark; --ink: #f5f1e8; --muted: #9d9a91; --line: #2c2d2a; --panel: #171815; --accent: #d6ff74; }
* { box-sizing: border-box; }
body { margin: 0; min-height: 100vh; background: #0d0e0c; color: var(--ink); font: 15px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; }
body::before { content: ""; position: fixed; inset: 0; pointer-events: none; background: radial-gradient(circle at 68% 18%, rgba(214,255,116,.07), transparent 32rem); }
main { width: min(92vw, 430px); margin: 11vh auto 4rem; position: relative; }
.mark { display: flex; align-items: center; gap: .7rem; margin-bottom: 2rem; letter-spacing: .08em; }
.prompt { color: var(--accent); }
.card { background: color-mix(in srgb, var(--panel) 94%, transparent); border: 1px solid var(--line); border-radius: 14px; padding: 1.5rem; box-shadow: 0 24px 80px rgba(0,0,0,.3); }
h1 { font: 500 1.35rem/1.3 inherit; margin: 0 0 .45rem; }
p { color: var(--muted); margin: 0 0 1.3rem; }
label { display: block; color: var(--muted); font-size: .78rem; margin: 1rem 0 .35rem; text-transform: uppercase; letter-spacing: .08em; }
input { width: 100%; color: var(--ink); background: #0f100e; border: 1px solid var(--line); border-radius: 8px; padding: .78rem .85rem; font: inherit; outline: none; }
input:focus { border-color: #788f47; box-shadow: 0 0 0 3px rgba(214,255,116,.08); }
button, .continue { width: 100%; border: 0; border-radius: 8px; padding: .82rem; margin-top: 1.2rem; background: var(--accent); color: #10120c; font: 600 .9rem/1 inherit; cursor: pointer; text-decoration: none; display: block; text-align: center; }
.quiet { color: var(--muted); background: transparent; border: 1px solid var(--line); }
.switch { margin-top: .9rem; color: var(--muted); text-align: center; font-size: .82rem; }
.switch button { display: inline; width: auto; border: 0; padding: 0; margin: 0; color: var(--accent); background: transparent; font: inherit; }
#message { min-height: 1.4rem; margin: 1rem 0 0; color: #ff9c86; font-size: .82rem; }
.identity { color: var(--ink); padding: .8rem 0 1rem; overflow-wrap: anywhere; }
.hidden { display: none; }
footer { color: #66685f; font-size: .72rem; margin-top: 1.2rem; text-align: center; }
`;

export const APP_JS = String.raw`
const form = document.querySelector("form");
const title = document.querySelector("h1");
const intro = document.querySelector("#intro");
const nameRow = document.querySelector("#name-row");
const toggle = document.querySelector("#toggle");
const submit = document.querySelector("#submit");
const message = document.querySelector("#message");
const signedOut = document.querySelector("#signed-out");
const signedIn = document.querySelector("#signed-in");
const identity = document.querySelector("#identity");
const continueLink = document.querySelector("#continue");
const returnTo = new URLSearchParams(location.search).get("return_to");
const safeReturn = returnTo && returnTo.startsWith("/v1/native/authorize?") ? returnTo : null;
let mode = "signin";

function setMode(next) {
  mode = next;
  const signup = mode === "signup";
  title.textContent = signup ? "Create your zmux account" : "Sign in to zmux";
  intro.textContent = signup ? "One account for your trusted zmux devices." : "Continue to your devices and native sessions.";
  nameRow.classList.toggle("hidden", !signup);
  submit.textContent = signup ? "Create account" : "Sign in";
  toggle.textContent = signup ? "Sign in instead" : "Create an account";
  message.textContent = "";
}

toggle.addEventListener("click", () => setMode(mode === "signin" ? "signup" : "signin"));
form.addEventListener("submit", async (event) => {
  event.preventDefault();
  message.textContent = "";
  submit.disabled = true;
  const fields = new FormData(form);
  const body = { email: fields.get("email"), password: fields.get("password") };
  if (mode === "signup") body.name = fields.get("name");
  try {
    const response = await fetch("/api/auth/" + (mode === "signup" ? "sign-up" : "sign-in") + "/email", {
      method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body)
    });
    const result = await response.json().catch(() => ({}));
    if (!response.ok) throw new Error(result.message || "Authentication failed");
    location.assign(safeReturn || "/");
  } catch (error) { message.textContent = error instanceof Error ? error.message : "Authentication failed"; }
  finally { submit.disabled = false; }
});

document.querySelector("#signout").addEventListener("click", async () => {
  await fetch("/api/auth/sign-out", { method: "POST" });
  location.assign("/");
});

const session = await fetch("/api/auth/get-session").then((response) => response.ok ? response.json() : null).catch(() => null);
if (session?.user) {
  signedOut.classList.add("hidden");
  signedIn.classList.remove("hidden");
  identity.textContent = session.user.email;
  if (safeReturn) { continueLink.href = safeReturn; continueLink.classList.remove("hidden"); }
}
`;

export function appPage(): string {
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>zmux cloud</title><link rel="stylesheet" href="/app.css"></head><body><main><div class="mark"><span class="prompt">▸_</span><span>zmux cloud</span></div><section class="card"><div id="signed-out"><h1>Sign in to zmux</h1><p id="intro">Continue to your devices and native sessions.</p><form><div id="name-row" class="hidden"><label for="name">Name</label><input id="name" name="name" maxlength="80" autocomplete="name"></div><label for="email">Email</label><input id="email" name="email" type="email" maxlength="254" autocomplete="email" required><label for="password">Password</label><input id="password" name="password" type="password" minlength="12" maxlength="128" autocomplete="current-password" required><button id="submit" type="submit">Sign in</button></form><div class="switch"><button id="toggle" type="button">Create an account</button></div><div id="message" role="alert"></div></div><div id="signed-in" class="hidden"><h1>Account connected</h1><p>Your browser session is active.</p><div id="identity" class="identity"></div><a id="continue" class="continue hidden" href="#">Continue to zmux</a><button id="signout" class="quiet" type="button">Sign out</button></div></section><footer>Identity and device control for zmux.</footer></main><script type="module" src="/app.js"></script></body></html>`;
}
