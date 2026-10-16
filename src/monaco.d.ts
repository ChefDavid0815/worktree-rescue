// Deep ESM entry points of monaco-editor used to keep the bundle lean.
declare module "monaco-editor/editor/editor.api.js" {
  export * from "monaco-editor";
}
declare module "monaco-editor/basic-languages/monaco.contribution.js";
declare module "monaco-editor/editor/editor.worker.js?worker" {
  const WorkerFactory: new () => Worker;
  export default WorkerFactory;
}
