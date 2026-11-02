import * as fs from "node:fs";
import * as path from "node:path";
import { spawn } from "node:child_process";

const [hostPath, rootSession, reportPath, ...providerArgs] = process.argv.slice(2);

if (providerArgs[0] === "--help") {
  const supportsStandalone = process.env.FIXTURE_OPENCODE_STANDALONE_SUPPORT !== "0";
  process.stdout.write(
    supportsStandalone
      ? "FLAGS\n  --standalone  Run with a private server\n"
      : "FLAGS\n  --server string  Connect to a server\n",
  );
  process.exit(0);
}

const configDirectory = process.env.OPENCODE_CONFIG_DIR ?? "";
const originalConfigDirectory = process.env.FIXTURE_OPENCODE_USER_CONFIG ?? "";
const configPath = path.join(configDirectory, "opencode.json");
const userPluginPath = path.join(configDirectory, "plugins", "user-plugin.js");
const pluginsDirectory = path.join(configDirectory, "plugins");
const pluginNames = fs.existsSync(pluginsDirectory) ? fs.readdirSync(pluginsDirectory) : [];
const zmuxPlugins = pluginNames.filter((name) => /^zmux-provider-[a-f0-9]+\.js$/u.test(name));
const expectedConfig = process.env.FIXTURE_OPENCODE_USER_CONFIG_CONTENT ?? "";
const expectedPlugin = process.env.FIXTURE_OPENCODE_USER_PLUGIN_CONTENT ?? "";
const configIsSymlink = fs.existsSync(configPath) && fs.lstatSync(configPath).isSymbolicLink();
const userPluginIsSymlink = fs.existsSync(userPluginPath) && fs.lstatSync(userPluginPath).isSymbolicLink();
const report = {
  args: providerArgs,
  config_directory_is_invocation: configDirectory !== originalConfigDirectory,
  config_is_symlink: configIsSymlink,
  config_preserved: fs.existsSync(configPath) && fs.readFileSync(configPath, "utf8") === expectedConfig,
  user_plugin_is_symlink: userPluginIsSymlink,
  user_plugin_preserved:
    fs.existsSync(userPluginPath) && fs.readFileSync(userPluginPath, "utf8") === expectedPlugin,
  zmux_plugin_count: zmuxPlugins.length,
  zmux_plugin_mode: zmuxPlugins.length === 1
    ? fs.statSync(path.join(pluginsDirectory, zmuxPlugins[0])).mode & 0o777
    : null,
};

fs.appendFileSync(process.env.FIXTURE_OPENCODE_CALLS, JSON.stringify(report) + "\n");
fs.writeFileSync(reportPath, JSON.stringify(report));

if (!providerArgs.includes("--standalone")) {
  process.stdout.write("OPENCODE_FALLBACK_LAUNCHED\n");
  process.exit(0);
}
if (zmuxPlugins.length !== 1) process.exit(73);

const routeMode = process.env.FIXTURE_OPENCODE_CAPTURE_ROUTE === "1" ? "capture" : "desktop";
const child = spawn(
  process.execPath,
  [hostPath, path.join(pluginsDirectory, zmuxPlugins[0]), process.env.ZMUX_PROVIDER_EVENT_SOCKET ?? "", rootSession, routeMode],
  { stdio: "inherit" },
);
child.once("error", (error) => {
  process.stderr.write(`synthetic OpenCode plugin host failed: ${error.message}\n`);
  process.exitCode = 74;
});
child.once("exit", (code) => {
  process.exitCode = code ?? 1;
});
