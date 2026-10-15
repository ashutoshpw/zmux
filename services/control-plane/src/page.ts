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
input, textarea { width: 100%; color: var(--ink); background: #0f100e; border: 1px solid var(--line); border-radius: 8px; padding: .78rem .85rem; font: inherit; outline: none; }
input:focus, textarea:focus { border-color: #788f47; box-shadow: 0 0 0 3px rgba(214,255,116,.08); }
button, .continue { width: 100%; border: 0; border-radius: 8px; padding: .82rem; margin-top: 1.2rem; background: var(--accent); color: #10120c; font: 600 .9rem/1 inherit; cursor: pointer; text-decoration: none; display: block; text-align: center; }
.quiet { color: var(--muted); background: transparent; border: 1px solid var(--line); }
.switch { margin-top: .9rem; color: var(--muted); text-align: center; font-size: .82rem; }
.switch button { display: inline; width: auto; border: 0; padding: 0; margin: 0; color: var(--accent); background: transparent; font: inherit; }
.message { min-height: 1.4rem; margin: 1rem 0 0; color: #ff9c86; font-size: .82rem; }
.message[data-kind="success"] { color: var(--accent); }
.identity { color: var(--ink); padding: .8rem 0 1rem; overflow-wrap: anywhere; }
textarea { resize: vertical; min-height: 5rem; }
button:disabled { opacity: .55; cursor: wait; }
h2 { font-size: 1rem; margin: 1.5rem 0 .45rem; }
.security { border-top: 1px solid var(--line); margin-top: 1.5rem; }
.secret { overflow-wrap: anywhere; font-size: .85rem; color: var(--ink); }
.codes { white-space: pre-wrap; padding: .8rem; border: 1px solid var(--line); border-radius: 8px; color: var(--ink); }
.ack { display: flex; align-items: flex-start; gap: .6rem; text-transform: none; letter-spacing: normal; font-size: .85rem; }
.ack input { width: auto; margin-top: .3rem; flex: none; }
.hidden { display: none; }
footer { color: #66685f; font-size: .72rem; margin-top: 1.2rem; text-align: center; }
`;

export const APP_JS = String.raw`
const $ = (selector) => document.querySelector(selector);
const form = $("#auth-form");
const title = $("#auth-title");
const intro = $("#intro");
const toggle = $("#toggle");
const submit = $("#submit");
const message = $("#message");
const signedOut = $("#signed-out");
const signedIn = $("#signed-in");
const continueLink = $("#continue");
const challenge = $("#challenge");
const challengeForm = $("#challenge-form");
const challengeCode = $("#challenge-code");
const challengeMessage = $("#challenge-message");
const securityForm = $("#security-form");
const enrollmentForm = $("#enrollment-form");
const securityMessage = $("#security-message");
const savedCodes = $("#saved-codes");
const signout = $("#signout");
const returnTo = new URLSearchParams(location.search).get("return_to");
const safeReturn = returnTo && returnTo.startsWith("/v1/native/authorize?") ? returnTo : null;
let mode = "signin";
let challengeMethod = "totp";
let twoFactorEnabled = false;
let enrollment = false;
let securityBusy = false;
let authBusy = false;
let challengeBusy = false;

async function authRequest(path, body) {
  const response = await fetch("/api/auth/" + path, {
    method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body)
  });
  const result = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(result.message || "The request could not be completed. Please try again.");
  return result;
}

function showError(target, error) {
  target.dataset.kind = "error";
  target.textContent = error instanceof Error ? error.message : "The request could not be completed. Please try again.";
}

function setMode(next) {
  mode = next;
  const signup = mode === "signup";
  title.textContent = signup ? "Create your zmux account" : "Sign in to zmux";
  intro.textContent = signup ? "One account for your trusted zmux devices." : "Continue to your devices and native sessions.";
  $("#name-row").classList.toggle("hidden", !signup);
  $("#password").autocomplete = signup ? "new-password" : "current-password";
  submit.textContent = signup ? "Create account" : "Sign in";
  toggle.textContent = signup ? "Sign in instead" : "Create an account";
  message.textContent = "";
}

function setChallengeMethod(next) {
  challengeMethod = next;
  const backup = next === "backup";
  $("#challenge-title").textContent = backup ? "Use a recovery code" : "Verify your sign-in";
  $("#challenge-intro").textContent = backup ? "Enter one unused recovery code. Each code can be used once." : "Enter the six-digit code from your authenticator app.";
  $("#challenge-label").textContent = backup ? "Recovery code" : "Authenticator code";
  $("#challenge-switch").textContent = backup ? "Use authenticator instead" : "Use a recovery code";
  challengeCode.inputMode = backup ? "text" : "numeric";
  challengeCode.maxLength = backup ? 128 : 6;
  challengeCode.pattern = backup ? ".*\\S.*" : "[0-9]{6}";
  challengeCode.autocomplete = backup ? "off" : "one-time-code";
  challengeCode.value = "";
  challengeMessage.textContent = "";
  challengeCode.focus();
}

function updateSecurityControls() {
  const blocked = enrollment || securityBusy;
  continueLink.classList.toggle("hidden", !safeReturn || blocked);
  signout.disabled = securityBusy || (enrollment && !savedCodes.checked);
  $("#security-submit").disabled = securityBusy;
  $("#enrollment-submit").disabled = securityBusy || !savedCodes.checked;
  $("#cancel-enrollment").disabled = securityBusy;
}

function renderSecurity() {
  $("#security-status").textContent = twoFactorEnabled ? "Two-factor authentication is enabled." : "Two-factor authentication is not enabled.";
  $("#security-intro").textContent = twoFactorEnabled ? "Enter your password to turn off authenticator and recovery-code verification." : "Add an authenticator app and save recovery codes to protect your account.";
  $("#security-submit").textContent = twoFactorEnabled ? "Disable two-factor authentication" : "Set up two-factor authentication";
  securityForm.classList.toggle("hidden", enrollment);
  $("#enrollment").classList.toggle("hidden", !enrollment);
  updateSecurityControls();
}

function clearEnrollment() {
  enrollment = false;
  savedCodes.checked = false;
  $("#manual-secret").textContent = "";
  $("#totp-uri").value = "";
  $("#backup-codes").textContent = "";
  $("#enrollment-code").value = "";
  renderSecurity();
}

function clearSensitiveInputs() {
  document.querySelectorAll('input[type="password"], #challenge-code, #enrollment-code').forEach((input) => { input.value = ""; });
  clearEnrollment();
}

function showAccount(user) {
  signedOut.classList.add("hidden");
  challenge.classList.add("hidden");
  signedIn.classList.remove("hidden");
  $("#identity").textContent = user.email;
  twoFactorEnabled = user.twoFactorEnabled === true;
  if (safeReturn) continueLink.href = safeReturn;
  renderSecurity();
}

toggle.addEventListener("click", () => setMode(mode === "signin" ? "signup" : "signin"));
form.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (authBusy) return;
  authBusy = true;
  message.textContent = "";
  submit.disabled = true;
  toggle.disabled = true;
  const fields = new FormData(form);
  const body = { email: fields.get("email"), password: fields.get("password") };
  if (mode === "signup") body.name = fields.get("name");
  try {
    const result = await authRequest((mode === "signup" ? "sign-up" : "sign-in") + "/email", body);
    $("#password").value = "";
    if (result.twoFactorRedirect === true) {
      signedOut.classList.add("hidden");
      challenge.classList.remove("hidden");
      setChallengeMethod("totp");
      return;
    }
    location.assign(safeReturn || "/");
  } catch (error) { showError(message, error); }
  finally { authBusy = false; submit.disabled = false; toggle.disabled = false; }
});

$("#challenge-switch").addEventListener("click", () => setChallengeMethod(challengeMethod === "totp" ? "backup" : "totp"));
$("#challenge-back").addEventListener("click", () => {
  challengeCode.value = "";
  challenge.classList.add("hidden");
  signedOut.classList.remove("hidden");
  $("#password").focus();
});
challengeForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (challengeBusy) return;
  challengeBusy = true;
  challengeMessage.textContent = "";
  const buttons = challenge.querySelectorAll("button");
  buttons.forEach((button) => { button.disabled = true; });
  try {
    await authRequest("two-factor/" + (challengeMethod === "backup" ? "verify-backup-code" : "verify-totp"), { code: challengeCode.value.trim() });
    challengeCode.value = "";
    location.assign(safeReturn || "/");
  } catch (error) { showError(challengeMessage, error); }
  finally { challengeBusy = false; buttons.forEach((button) => { button.disabled = false; }); }
});

securityForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (securityBusy || enrollment) return;
  securityMessage.textContent = "";
  securityBusy = true;
  updateSecurityControls();
  try {
    const password = $("#security-password").value;
    if (twoFactorEnabled) {
      await authRequest("two-factor/disable", { password });
      twoFactorEnabled = false;
      securityMessage.dataset.kind = "success";
      securityMessage.textContent = "Two-factor authentication has been disabled.";
    } else {
      const result = await authRequest("two-factor/enable", { password, method: "totp" });
      const uri = new URL(result.totpURI);
      const secret = uri.searchParams.get("secret");
      if (uri.protocol !== "otpauth:" || uri.hostname !== "totp" || !secret || !Array.isArray(result.backupCodes) || !result.backupCodes.length || result.backupCodes.some((code) => typeof code !== "string")) {
        throw new Error("Setup details were incomplete. Please try setting up two-factor authentication again.");
      }
      $("#manual-secret").textContent = secret;
      $("#totp-uri").value = result.totpURI;
      $("#backup-codes").textContent = result.backupCodes.join("\n");
      savedCodes.checked = false;
      enrollment = true;
    }
  } catch (error) { showError(securityMessage, error); }
  finally {
    $("#security-password").value = "";
    securityBusy = false;
    renderSecurity();
    if (enrollment) $("#enrollment-code").focus();
  }
});

savedCodes.addEventListener("change", updateSecurityControls);
enrollmentForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (securityBusy || !enrollment || !savedCodes.checked) return;
  securityMessage.textContent = "";
  securityBusy = true;
  updateSecurityControls();
  try {
    await authRequest("two-factor/verify-totp", { code: $("#enrollment-code").value.trim() });
    twoFactorEnabled = true;
    clearEnrollment();
    securityMessage.dataset.kind = "success";
    securityMessage.textContent = "Two-factor authentication is enabled. Keep your recovery codes safe.";
  } catch (error) { showError(securityMessage, error); }
  finally { securityBusy = false; updateSecurityControls(); }
});

$("#cancel-enrollment").addEventListener("click", () => {
  if (securityBusy || !enrollment) return;
  if (!savedCodes.checked && !confirm("Discard these recovery codes and cancel setup? Two-factor authentication has not been enabled.")) return;
  clearEnrollment();
  securityMessage.dataset.kind = "success";
  securityMessage.textContent = "Setup cancelled. Two-factor authentication has not been enabled.";
});

window.addEventListener("beforeunload", (event) => {
  if (enrollment && !savedCodes.checked) {
    event.preventDefault();
    event.returnValue = "";
  }
});
signout.addEventListener("click", async () => {
  if (securityBusy || (enrollment && !savedCodes.checked)) return;
  securityBusy = true;
  securityMessage.textContent = "";
  updateSecurityControls();
  try {
    await authRequest("sign-out", {});
    clearSensitiveInputs();
    location.assign("/");
  } catch (error) { securityBusy = false; showError(securityMessage, error); updateSecurityControls(); }
});

const session = await fetch("/api/auth/get-session").then((response) => response.ok ? response.json() : null).catch(() => null);
if (session?.user) showAccount(session.user);
submit.disabled = false;
`;

export function appPage(): string {
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>zmux cloud</title><link rel="stylesheet" href="/app.css"></head>
<body><main><div class="mark"><span class="prompt">▸_</span><span>zmux cloud</span></div><section class="card">
<div id="signed-out"><h1 id="auth-title">Sign in to zmux</h1><p id="intro">Continue to your devices and native sessions.</p>
<form id="auth-form"><div id="name-row" class="hidden"><label for="name">Name</label><input id="name" name="name" maxlength="80" autocomplete="name"></div>
<label for="email">Email</label><input id="email" name="email" type="email" maxlength="254" autocomplete="email" required>
<label for="password">Password</label><input id="password" name="password" type="password" minlength="12" maxlength="128" autocomplete="current-password" required>
<button id="submit" type="submit" disabled>Sign in</button></form><div class="switch"><button id="toggle" type="button">Create an account</button></div><div id="message" class="message" role="alert"></div></div>
<div id="challenge" class="hidden"><h1 id="challenge-title">Verify your sign-in</h1><p id="challenge-intro">Enter the six-digit code from your authenticator app.</p>
<form id="challenge-form"><label id="challenge-label" for="challenge-code">Authenticator code</label><input id="challenge-code" name="code" inputmode="numeric" autocomplete="one-time-code" maxlength="6" pattern="[0-9]{6}" spellcheck="false" required><button type="submit">Verify and continue</button></form>
<div class="switch"><button id="challenge-switch" type="button">Use a recovery code</button></div><button id="challenge-back" class="quiet" type="button">Back to sign in</button><div id="challenge-message" class="message" role="alert"></div></div>
<div id="signed-in" class="hidden"><h1>Account connected</h1><p>Your browser session is active.</p><div id="identity" class="identity"></div><a id="continue" class="continue hidden" href="#">Continue to zmux</a>
<section class="security" aria-labelledby="security-title"><h2 id="security-title">Account security</h2><p id="security-status"></p>
<form id="security-form"><p id="security-intro"></p><label for="security-password">Confirm password</label><input id="security-password" type="password" minlength="12" maxlength="128" autocomplete="current-password" required><button id="security-submit" type="submit">Set up two-factor authentication</button></form>
<div id="enrollment" class="hidden"><h2>Set up your authenticator</h2><p>Add this manual setup key to your authenticator app, or import the full setup URI. Two-factor authentication stays off until you verify a code.</p>
<label>Manual setup key</label><div id="manual-secret" class="secret" role="note" aria-label="Manual setup key"></div><label for="totp-uri">Authenticator setup URI</label><textarea id="totp-uri" readonly autocomplete="off" spellcheck="false"></textarea>
<h2>Save your recovery codes</h2><p>Store these codes somewhere safe before continuing. Each code can be used once if you lose access to your authenticator.</p><pre id="backup-codes" class="codes" aria-label="Recovery codes"></pre>
<form id="enrollment-form"><label class="ack" for="saved-codes"><input id="saved-codes" type="checkbox" required>I have saved my recovery codes somewhere safe.</label><label for="enrollment-code">Authenticator code</label><input id="enrollment-code" inputmode="numeric" autocomplete="one-time-code" maxlength="6" pattern="[0-9]{6}" spellcheck="false" required><button id="enrollment-submit" type="submit" disabled>Verify and enable</button></form><button id="cancel-enrollment" class="quiet" type="button">Cancel setup</button></div>
<div id="security-message" class="message" role="status" aria-live="polite"></div></section><button id="signout" class="quiet" type="button">Sign out</button></div>
</section><footer>Identity and device control for zmux.</footer></main><script type="module" src="/app.js"></script></body></html>`;
}
