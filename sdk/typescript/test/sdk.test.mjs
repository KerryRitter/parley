import assert from "node:assert/strict";
import { chmod, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { ParleyClient, openAICompatibleEnv } from "../dist/index.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const par = process.env.PARLEY_BIN ?? path.resolve(here, "../../../target/debug/par");

test("discovers normalized harness capabilities", async () => {
  const client = new ParleyClient({ binary: par });
  const capabilities = await client.capabilities("co");
  assert.equal(capabilities.harness, "codex");
  assert.equal(capabilities.environmentOverride, true);
  assert.equal(capabilities.cancellation, true);
});

test("passes executable and OpenAI-compatible environment overrides", async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "parley-sdk-"));
  const executable = path.join(directory, "fake-agent");
  await writeFile(
    executable,
    `#!/bin/sh
if [ "$OPENAI_API_KEY" = "sdk-test-key" ]; then key=present; else key=missing; fi
if [ -z "$SHOULD_BE_UNSET" ]; then unset_value=yes; else unset_value=no; fi
yolo=no
for arg in "$@"; do [ "$arg" = "--dangerously-bypass-approvals-and-sandbox" ] && yolo=yes; done
printf 'base=%s key=%s unset=%s yolo=%s' "$OPENAI_BASE_URL" "$key" "$unset_value" "$yolo"
`,
  );
  await chmod(executable, 0o755);

  try {
    const client = new ParleyClient({
      binary: par,
      processEnv: { SHOULD_BE_UNSET: "inherited" },
    });
    const run = client.start({
      harness: "codex",
      prompt: "ignored by fake executable",
      executable,
      env: openAICompatibleEnv({
        baseUrl: "http://127.0.0.1:52415/v1",
        apiKey: "sdk-test-key",
      }),
      unsetEnv: ["SHOULD_BE_UNSET"],
    });
    const eventTypes = [];
    for await (const event of run) eventTypes.push(event.type);
    const result = await run.result();
    assert.equal(result.status, "completed");
    assert.equal(
      result.output,
      "base=http://127.0.0.1:52415/v1 key=present unset=yes yolo=no",
    );
    assert.deepEqual(eventTypes, ["started", "stdout", "completed"]);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("cancels the whole agent run", async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "parley-sdk-cancel-"));
  const executable = path.join(directory, "slow-agent");
  await writeFile(executable, "#!/bin/sh\nexec 1>&- 2>&-\nsleep 30\n");
  await chmod(executable, 0o755);

  try {
    const client = new ParleyClient({ binary: par });
    const run = client.start({ harness: "codex", prompt: "wait", executable });
    setTimeout(() => run.cancel(), 100);
    const result = await run.result();
    assert.equal(result.status, "cancelled");
    assert.equal(result.terminationReason, "cancelled");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("reports whether the idle watchdog timed out", async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "parley-sdk-timeout-"));
  const executable = path.join(directory, "silent-agent");
  await writeFile(executable, "#!/bin/sh\nexec 1>&- 2>&-\nsleep 30\n");
  await chmod(executable, 0o755);

  try {
    const client = new ParleyClient({ binary: par });
    const result = await client.run({
      harness: "codex",
      prompt: "wait",
      executable,
      timeout: { overallMs: 5_000, idleMs: 80 },
    });
    assert.equal(result.status, "timed_out");
    assert.equal(result.terminationReason, "idle_timeout");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
