/** Side-effect CSS imports (the ds primitives pull their stylesheet). The host's
 *  bundler injects the CSS; for tsc these modules just need to type-resolve. This
 *  keeps src/ds self-contained for the package's types-only `/ui` emit, which
 *  compiles these files outside the app's vite/client ambient types. */
declare module "*.css";
