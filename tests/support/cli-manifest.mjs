// The rules of docs/cli.md "Plugins and the command line" for a manifest's `cli`
// section, as a function: returns the list of problems (empty when it is valid).

const WORD = /^[a-z][a-z0-9-]*$/;
const KEBAB = /^[a-z][a-z0-9]*(-[a-z0-9]+)*$/;
const TYPES = ["string", "number", "flag", "choice", "path"];
const camel = (name) => name.replace(/-([a-z0-9])/g, (_, c) => c.toUpperCase());

function checkParams(kind, owner, params, problems, { allowFlag }) {
  if (params === undefined) return;
  if (!Array.isArray(params)) { problems.push(`${owner}: ${kind} must be a list`); return; }
  const seen = new Set();
  for (const param of params) {
    const where = `${owner} ${kind} ${param?.name}`;
    if (typeof param?.name !== "string" || !KEBAB.test(param.name)) { problems.push(`${where}: name must be kebab-case`); continue; }
    if (seen.has(camel(param.name))) problems.push(`${where}: name used twice`);
    seen.add(camel(param.name));
    if (!TYPES.includes(param.type)) { problems.push(`${where}: unknown type ${param.type}`); continue; }
    if (!allowFlag && param.type === "flag") problems.push(`${where}: an argument cannot be a flag`);
    if (param.type === "choice" && !(Array.isArray(param.choices) && param.choices.length && param.choices.every((c) => typeof c === "string"))) problems.push(`${where}: a choice needs choices`);
    if (param.type !== "choice" && param.choices !== undefined) problems.push(`${where}: only a choice has choices`);
    if (param.default !== undefined) {
      const ok = param.type === "flag" ? typeof param.default === "boolean"
        : param.type === "number" ? typeof param.default === "number"
        : typeof param.default === "string" && (param.type !== "choice" || (param.choices ?? []).includes(param.default));
      if (!ok) problems.push(`${where}: default ${JSON.stringify(param.default)} does not fit the type`);
    }
    if (param.required && param.default !== undefined) problems.push(`${where}: required and default together`);
    if (param.type === "flag" && (param.required || param.multiple)) problems.push(`${where}: a flag is neither required nor multiple`);
    if (param.help !== undefined && typeof param.help !== "string") problems.push(`${where}: help must be text`);
  }
}

/** Walk the commands; returns the runnable ones as the paths a module must handle. */
function checkCommands(commands, prefix, problems, runnable) {
  if (!Array.isArray(commands)) { problems.push(`${prefix || "cli"}: commands must be a list`); return; }
  const seen = new Set();
  for (const command of commands) {
    const full = [...prefix, command?.name];
    const where = `command ${full.join(" ")}`;
    if (typeof command?.name !== "string" || !WORD.test(command.name)) { problems.push(`${where}: name must match [a-z][a-z0-9-]*`); continue; }
    if (seen.has(command.name)) problems.push(`${where}: name used twice`);
    seen.add(command.name);
    if (typeof command.summary !== "string" || !command.summary) problems.push(`${where}: needs a summary`);
    const isGroup = command.commands !== undefined;
    const hasRunnableParts = command.args !== undefined || command.options !== undefined || command.selection !== undefined;
    if (isGroup && hasRunnableParts) problems.push(`${where}: a command is a group (commands) or runs (args/options), not both`);
    if (isGroup) {
      if (!Array.isArray(command.commands) || !command.commands.length) problems.push(`${where}: a group needs commands`);
      else checkCommands(command.commands, full, problems, runnable);
    } else {
      runnable.push(full.join(" "));
      checkParams("arg", where, command.args, problems, { allowFlag: false });
      checkParams("option", where, command.options, problems, { allowFlag: true });
      if (command.selection !== undefined && typeof command.selection !== "boolean") problems.push(`${where}: selection must be true or false`);
      // A multiple argument takes the rest of the line, so it must come last.
      (command.args ?? []).forEach((arg, at, all) => { if (arg.multiple && at !== all.length - 1) problems.push(`${where}: only the last argument can be multiple`); });
    }
  }
}

/**
 * @param manifest a parsed manifest.json
 * @param module the loaded cli module (resolved `PluginModule`), or undefined to check the manifest alone
 */
export function validateCli(manifest, module) {
  const problems = [];
  const cli = manifest.cli;
  if (cli === undefined) return problems;
  if (typeof cli.module !== "string" || !/\.js$/.test(cli.module) || cli.module.startsWith("/") || cli.module.includes("..")) problems.push("cli.module must be a plugin-relative .js path");

  const importers = cli.importers ?? [];
  if (!Array.isArray(importers)) problems.push("cli.importers must be a list");
  const importerNames = new Set();
  for (const importer of Array.isArray(importers) ? importers : []) {
    const where = `importer ${importer?.name}`;
    if (typeof importer?.name !== "string" || !WORD.test(importer.name)) { problems.push(`${where}: name must match [a-z][a-z0-9-]*`); continue; }
    if (importerNames.has(importer.name)) problems.push(`${where}: name used twice`);
    importerNames.add(importer.name);
    if (typeof importer.summary !== "string" || !importer.summary) problems.push(`${where}: needs a summary`);
    checkParams("option", where, importer.options, problems, { allowFlag: true });
  }

  if (cli.exporter !== undefined) {
    const exporter = cli.exporter;
    const where = `exporter ${exporter?.name}`;
    if (typeof exporter?.name !== "string" || !WORD.test(exporter.name)) problems.push(`${where}: name must match [a-z][a-z0-9-]*`);
    if (typeof exporter?.summary !== "string" || !exporter?.summary) problems.push(`${where}: needs a summary`);
    checkParams("option", where, exporter?.options, problems, { allowFlag: true });
    if (exporter?.options === undefined && !(Array.isArray(manifest.fields) && manifest.fields.length)) problems.push(`${where}: no options, and no manifest fields to take them from`);
  }

  const runnable = [];
  if (cli.commands !== undefined) checkCommands(cli.commands, [], problems, runnable);

  if (module !== undefined) {
    const handlers = module?.cli ?? {};
    const have = Object.keys(handlers.importers ?? {});
    for (const name of importerNames) if (typeof handlers.importers?.[name] !== "function") problems.push(`importer ${name}: the module has no handler`);
    for (const name of have) if (!importerNames.has(name)) problems.push(`importer ${name}: the module handles it but the manifest does not declare it`);
    const commandHandlers = Object.keys(handlers.commands ?? {});
    for (const path of runnable) if (typeof handlers.commands?.[path] !== "function") problems.push(`command ${path}: the module has no handler`);
    for (const path of commandHandlers) if (!runnable.includes(path)) problems.push(`command ${path}: the module handles it but the manifest does not declare a runnable command of that path`);
    if (cli.exporter !== undefined && typeof module?.run !== "function") problems.push(`exporter ${cli.exporter.name}: the module has no run`);
  }
  return problems;
}
