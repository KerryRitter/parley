# Parley SDK protocol v1

The TypeScript SDK talks to the dependency-free Rust runtime through a small,
language-neutral JSONL protocol. This keeps orchestration in the caller while
Parley owns adapter translation and child-process supervision.

## Commands

```sh
par sdk capabilities
par sdk capabilities --harness codex
par sdk run
```

`capabilities` writes one JSON document and exits. `run` reads JSONL from stdin
and writes JSONL events to stdout. Protocol diagnostics go to stderr; agent
stderr is a structured `stderr` event on stdout.

## Run request

The first stdin line is the request:

```json
{"protocolVersion":1,"harness":"codex","prompt":"Implement task 3","model":"gpt-5.6-luna","cwd":"/workspace","executable":"exo-codex","env":{"OPENAI_BASE_URL":"http://127.0.0.1:52415/v1","OPENAI_API_KEY":"local-key"},"unsetEnv":["OLD_OPENAI_KEY"],"inheritEnv":true,"timeout":{"overallMs":1800000,"idleMs":300000},"maxCaptureChars":80000,"yolo":false}
```

Only `harness` and `prompt` are required. Supported optional fields are:

| Field | Meaning |
| --- | --- |
| `provider`, `model`, `agent` | Shared adapter selection fields |
| `cwd` | Child working directory |
| `outputFormat`, `inputFormat` | Native format flags when supported |
| `permissionMode`, `maxTurns` | Native permission/turn controls when supported |
| `sessionId`, `resumeId` | Native session continuity when supported |
| `passthrough` | Final raw arguments appended to the adapter invocation |
| `executable` | Replaces the adapter's normal executable, preserving its translated arguments |
| `env` | String environment values applied after adapter-provided values |
| `unsetEnv` | Environment names to remove after adapter defaults; explicit `env` values win |
| `inheritEnv` | Whether to inherit the Parley process environment; defaults to `true` |
| `timeout.overallMs` | Wall-clock timeout; zero disables it |
| `timeout.idleMs` | No-output timeout; zero disables it |
| `maxCaptureChars` | Byte limit for each final output tail; events remain unbounded |
| `yolo` | Permission bypass; defaults to `false` on this protocol |

Environment values and the prompt arrive over stdin rather than process argv.
They are never included in Parley's protocol events. They are inherited by the
selected agent and its descendants, and a downstream agent can still print
them, so its output must be treated as sensitive.

`inheritEnv: false` is an allowlist mode. Supply every variable the executable
needs, including `PATH` if `executable` is not an absolute path.

## Events

Every event includes `protocolVersion: 1`.

```json
{"protocolVersion":1,"type":"started","harness":"codex","pid":1234}
{"protocolVersion":1,"type":"stdout","sequence":1,"data":"working...\n"}
{"protocolVersion":1,"type":"stderr","sequence":2,"data":"diagnostic\n"}
{"protocolVersion":1,"type":"completed","result":{"status":"completed","terminationReason":null,"exitCode":0,"output":"working...\n","stderr":"diagnostic\n","durationMs":812,"sessionId":null}}
```

`status` is `completed`, `failed`, `cancelled`, or `timed_out`.
`terminationReason` distinguishes `cancelled`, `overall_timeout`, and
`idle_timeout` terminations and is otherwise null. Agent failures
still produce a normal terminal event; transport or request errors make `par`
exit nonzero and write a diagnostic to process stderr.

The `output` and `stderr` result fields contain bounded tails for callers that
do not consume streams. Consume the events when complete output matters.

## Control messages

After the request, the caller may write:

```json
{"type":"cancel"}
```

On Unix, Parley starts the agent in a separate process group and terminates the
group. On Windows it uses tree termination. The terminal result has status
`cancelled`. Closing stdin does not cancel a run.

Live steering is not part of v1. Capability discovery reports
`liveSteering: false`; a future protocol version can add it without pretending
that every downstream CLI accepts mid-run input.

## Capability discovery

`par sdk capabilities` returns `protocolVersion` plus a `harnesses` array.
Passing `--harness` returns one normalized capability object. Use this to decide
whether a plan item may request provider selection, structured output, resume,
or another native feature. Runtime controls (`environmentOverride`,
`executableOverride`, and `cancellation`) are available for every harness.

## Compatibility

Callers must send `protocolVersion: 1` and reject event versions they do not
understand. Parsers should tolerate unknown object fields so v1 can gain
backward-compatible metadata. New event variants or semantic changes require a
new protocol version.
