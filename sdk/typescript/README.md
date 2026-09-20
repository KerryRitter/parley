# `@parley/sdk`

Typed Node.js access to Parley's Rust agent runtime. Install the `par` binary,
then construct a client (or set `PARLEY_BIN` to its path):

```ts
import { ParleyClient, openAICompatibleEnv } from "@parley/sdk";

const parley = new ParleyClient();
const run = parley.start({
  harness: "codex",
  model: "gpt-5.6-luna",
  prompt: "Implement the next item in engineering-plan.md",
  executable: "exo-codex",
  env: openAICompatibleEnv({
    baseUrl: "http://127.0.0.1:52415/v1",
    apiKey: process.env.EXO_API_KEY,
  }),
  cwd: process.cwd(),
});

for await (const event of run) {
  if (event.type === "stdout") process.stdout.write(event.data);
}

const result = await run.result();
```

`env` is sent to Parley over stdin, not on the process command line. It is
applied only to the selected agent process. Use `unsetEnv` to remove child
variables or `inheritEnv: false` for an allowlisted environment. SDK runs do not
enable permission bypass unless `yolo: true` is explicit.

`ParleyRun` is an `AsyncIterable` of `started`, `stdout`, `stderr`, and
`completed` events. Call `cancel()` or pass an `AbortSignal`; call `result()`
for the bounded final output tails. `ParleyClient.capabilities()` exposes the
native features of every registered adapter. A timed-out result reports
`terminationReason` as `overall_timeout` or `idle_timeout`.
