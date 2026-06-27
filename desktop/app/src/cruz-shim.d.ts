// @cruzjs/ui ships TS source but its package `exports` only maps "./*" without a
// `types` condition, so TypeScript can't resolve per-component subpaths (Vite's
// esbuild resolves the real source fine at build time). This shim lets tsc
// typecheck the rest of the app; component props come through as `any`.
declare module "@cruzjs/ui/components/*";
