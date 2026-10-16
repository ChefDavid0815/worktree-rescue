// Loaded lazily: Monaco is only needed once a diff is opened.
import * as monaco from "monaco-editor/editor/editor.api.js";
import "monaco-editor/basic-languages/monaco.contribution.js";
import EditorWorker from "monaco-editor/editor/editor.worker.js?worker";
import { DiffEditor, loader } from "@monaco-editor/react";

declare global {
  interface Window {
    MonacoEnvironment?: { getWorker: (id: string, label: string) => Worker };
  }
}

self.MonacoEnvironment = { getWorker: () => new EditorWorker() };
loader.config({ monaco });

monaco.editor.defineTheme("wr-dark", {
  base: "vs-dark",
  inherit: true,
  rules: [
    { token: "comment", foreground: "6B7A8F", fontStyle: "italic" },
    { token: "keyword", foreground: "8FC9F2" },
    { token: "string", foreground: "B8D59A" },
    { token: "number", foreground: "E0B47A" },
    { token: "type", foreground: "7FE2EA" },
  ],
  colors: {
    "editor.background": "#0E141C",
    "editor.foreground": "#D9E1EA",
    "editorLineNumber.foreground": "#3C4A5D",
    "editorLineNumber.activeForeground": "#8A99AD",
    "editor.lineHighlightBackground": "#141C27",
    "editor.selectionBackground": "#5CD0DC33",
    "diffEditor.insertedTextBackground": "#5CD0DC24",
    "diffEditor.removedTextBackground": "#E26B6026",
    "diffEditor.insertedLineBackground": "#5CD0DC12",
    "diffEditor.removedLineBackground": "#E26B6012",
    "diffEditor.diagonalFill": "#1D273388",
    "scrollbarSlider.background": "#2A364680",
    "scrollbarSlider.hoverBackground": "#3A4A60A0",
    "editorGutter.background": "#0E141C",
  },
});
monaco.editor.defineTheme("wr-light", {
  base: "vs",
  inherit: true,
  rules: [{ token: "comment", foreground: "7A8698", fontStyle: "italic" }],
  colors: {
    "editor.background": "#FBFCFD",
    "editorLineNumber.foreground": "#B3BDC9",
    "diffEditor.insertedTextBackground": "#0E849422",
    "diffEditor.removedTextBackground": "#C0392F22",
    "diffEditor.insertedLineBackground": "#0E84940E",
    "diffEditor.removedLineBackground": "#C0392F0E",
  },
});

const LANG: Record<string, string> = {
  ts: "typescript", tsx: "typescript", js: "javascript", jsx: "javascript", mjs: "javascript", cjs: "javascript",
  json: "json", md: "markdown", rs: "rust", py: "python", go: "go", java: "java", kt: "kotlin", swift: "swift",
  c: "c", h: "c", cpp: "cpp", hpp: "cpp", cc: "cpp", cs: "csharp", rb: "ruby", php: "php", sh: "shell",
  bash: "shell", ps1: "powershell", yml: "yaml", yaml: "yaml", toml: "ini", ini: "ini", html: "html",
  css: "css", scss: "scss", less: "less", sql: "sql", xml: "xml", svg: "xml", vue: "html", svelte: "html",
  dockerfile: "dockerfile", lua: "lua", dart: "dart", r: "r", scala: "scala",
};

export function languageFor(path: string): string {
  const base = path.split("/").pop()?.toLowerCase() ?? "";
  if (base === "dockerfile") return "dockerfile";
  const ext = base.includes(".") ? base.split(".").pop()! : "";
  return LANG[ext] ?? "plaintext";
}

export default function MonacoDiff({
  original,
  modified,
  path,
  theme,
  sideBySide,
}: {
  original: string;
  modified: string;
  path: string;
  theme: "dark" | "light";
  sideBySide: boolean;
}) {
  return (
    <DiffEditor
      original={original}
      modified={modified}
      language={languageFor(path)}
      theme={theme === "dark" ? "wr-dark" : "wr-light"}
      options={{
        readOnly: true,
        originalEditable: false,
        renderSideBySide: sideBySide,
        minimap: { enabled: false },
        fontFamily: "'IBM Plex Mono', Consolas, monospace",
        fontSize: 12.5,
        lineHeight: 19,
        scrollBeyondLastLine: false,
        renderOverviewRuler: true,
        automaticLayout: true,
        hideUnchangedRegions: { enabled: true, contextLineCount: 4 },
        diffWordWrap: "off",
      }}
    />
  );
}
