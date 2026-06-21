/** Side-effect CSS imports (the ds primitives pull their stylesheet). The host's
 *  bundler injects the CSS; for tsc these modules just need to type-resolve. */
declare module "*.css";
