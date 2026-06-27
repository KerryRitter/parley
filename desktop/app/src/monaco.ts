// Monaco setup for Vite. We only wire the core editor worker (needed for diff
// computation); language IntelliSense workers are skipped — syntax highlighting
// is main-thread so the diff viewer still looks right without them.
import * as monaco from "monaco-editor";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";

self.MonacoEnvironment = {
  getWorker() {
    return new EditorWorker();
  },
};

monaco.editor.defineTheme("parley-dark", {
  base: "vs-dark",
  inherit: true,
  rules: [],
  colors: {
    "editor.background": "#0b0c0f",
    "editor.foreground": "#dfe3ea",
    "editorGutter.background": "#0b0c0f",
    "editorLineNumber.foreground": "#444b59",
    "editorLineNumber.activeForeground": "#8a90a0",
    "diffEditor.insertedTextBackground": "#2f6b4822",
    "diffEditor.removedTextBackground": "#c44b4222",
    "diffEditor.insertedLineBackground": "#16291e",
    "diffEditor.removedLineBackground": "#2a1414",
    "editor.lineHighlightBackground": "#14161d",
    "scrollbarSlider.background": "#2a2f3a66",
  },
});

export function langForPath(path: string): string {
  const ext = path.split(".").pop()?.toLowerCase() || "";
  const map: Record<string, string> = {
    ts: "typescript", tsx: "typescript", js: "javascript", jsx: "javascript",
    rs: "rust", py: "python", go: "go", rb: "ruby", java: "java", c: "c", h: "c",
    cpp: "cpp", cc: "cpp", cs: "csharp", json: "json", md: "markdown", css: "css",
    scss: "scss", html: "html", htm: "html", toml: "ini", yaml: "yaml", yml: "yaml",
    sh: "shell", bash: "shell", sql: "sql", php: "php", swift: "swift", kt: "kotlin",
  };
  return map[ext] || "plaintext";
}

export { monaco };
